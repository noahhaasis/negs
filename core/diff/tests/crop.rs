//! The crop, Odin core (negs_core_rules::crop) against the Rust oracle
//! (windows/src/crop.rs as it was, core/diff/oracle/crop.rs), and the
//! core's port of the engine's pixel rounding against spektrafilm-io's
//! `Crop::pixels` and `straightened_size` themselves. Every function on
//! seeded inputs (NEGS_RULES_SEEDS, default 10,000; NEGS_RULES_SEED), every
//! output field bit for bit; the crop session driven through the same
//! seeded steps in both, compared after each; the serde format (session
//! files and sidecars) byte for byte both ways.

mod picture_common;

use negs_core_diff_picture as oracle_crate;
use negs_core_rules::crop as core;
use negs_core_sys::picture as sys;
use oracle_crate::crop as oracle;
use picture_common::{Random, Tally, cases, gate, seed};

fn o_rect(r: core::Rect) -> oracle::Rect {
    oracle::Rect::new(r.x, r.y, r.width, r.height)
}
fn o_pt(p: core::Point) -> oracle::Point {
    oracle::Point { x: p.x, y: p.y }
}
fn o_size(s: core::Size) -> oracle::Size {
    oracle::Size::new(s.width, s.height)
}
fn o_aspect(a: core::CropAspect) -> oracle::CropAspect {
    oracle::CropAspect::ALL[core::CropAspect::ALL.iter().position(|x| *x == a).unwrap()]
}
fn o_settings(s: &core::CropSettings) -> oracle::CropSettings {
    oracle::CropSettings { rect: o_rect(s.rect), aspect: o_aspect(s.aspect), custom: o_size(s.custom), angle: s.angle }
}

fn rect(r: &mut Random) -> core::Rect {
    if r.chance(3) {
        core::Rect::new(r.fraction(), r.fraction(), r.fraction(), r.fraction())
    } else {
        core::Rect::new(r.length() - 300.0, r.length() - 300.0, r.length(), r.length())
    }
}
fn fraction_rect(r: &mut Random) -> core::Rect {
    if r.chance(8) {
        return core::Rect::UNIT;
    }
    let (x, y) = (r.fraction(), r.fraction());
    core::Rect::new(x, y, r.fraction() * (1.0 - x).abs().max(0.01), r.fraction() * (1.0 - y).abs().max(0.01))
}
fn point(r: &mut Random) -> core::Point {
    core::Point { x: r.length() - 200.0, y: r.length() - 200.0 }
}
fn size(r: &mut Random) -> core::Size {
    if r.chance(10) { core::Size::ZERO } else { core::Size::new(r.length(), r.length()) }
}
fn aspect(r: &mut Random) -> core::CropAspect {
    core::CropAspect::ALL[r.below(9) as usize]
}
fn settings(r: &mut Random) -> core::CropSettings {
    core::CropSettings {
        rect: fraction_rect(r),
        aspect: aspect(r),
        custom: if r.chance(2) { core::Size::new(5.0, 4.0) } else { core::Size::new(r.range(0.05, 20.0), r.range(0.05, 20.0)) },
        angle: if r.chance(2) { 0.0 } else { r.angle() },
    }
}
fn rotation(r: &mut Random) -> i32 {
    match r.below(10) {
        0 => i32::MIN + r.below(4) as i32,
        1 => i32::MAX - r.below(4) as i32,
        _ => r.int(-9, 9) as i32,
    }
}
fn orientation(r: &mut Random) -> core::Orientation {
    // Built by hand as well as by `new`: the fields are public.
    if r.chance(3) { core::Orientation { turns: r.int(-6, 9) as i32, flip: r.chance(2) } } else { core::Orientation::new(rotation(r), r.chance(2)) }
}
fn o_orientation(o: core::Orientation) -> oracle::Orientation {
    oracle::Orientation { turns: o.turns, flip: o.flip }
}

fn geometry(r: &mut Random, t: &mut Tally, i: usize) {
    let w = |what: &'static str| move || format!("case {i}: {what}");
    for a in core::CropAspect::ALL {
        let photo = size(r);
        t.eq(&w("title"), o_aspect(a).title(), a.title());
        t.eq(&w("format"), o_aspect(a).format(), a.format());
        t.dbg(&w("ratio"), o_aspect(a).ratio(o_size(photo)), a.ratio(photo));
        t.eq(&w("turns"), o_aspect(a).turns(), a.turns());
    }
    let s = settings(r);
    let os = o_settings(&s);
    let sz = if r.chance(4) { core::Size::new(r.below(9000) as f64, r.below(9000) as f64) } else { size(r) };
    t.eq(&w("cropped"), os.cropped(), s.cropped());
    t.dbg(&w("pixels"), os.pixels(o_size(sz)), o_rect(s.pixels(sz)));
    t.dbg(&w("size_of"), os.size_of(o_size(sz)), o_size(s.size_of(sz)));
    t.dbg(&w("engine"), os.engine(), s.engine());

    // The port of the engine's rounding against the engine itself.
    let (pw, ph) = (r.pixels().max(1), r.pixels().max(1));
    let engine = spektrafilm_io::crop::Crop { x: s.rect.x, y: s.rect.y, width: s.rect.width, height: s.rect.height, angle: s.angle };
    let c = sys::negs_crop_settings_t {
        rect: sys::negs_rect_t { x: s.rect.x, y: s.rect.y, w: s.rect.width, h: s.rect.height },
        custom: sys::negs_size_t { width: s.custom.width, height: s.custom.height },
        angle: s.angle,
        aspect: 0,
        reserved: 0,
    };
    let mut px = [0u32; 4];
    let mut st = [0u32; 2];
    unsafe {
        sys::negs_crop_engine_pixels(&c, pw, ph, &mut px);
        sys::negs_crop_straightened_size(&c, pw, ph, &mut st);
    }
    let e = engine.pixels(pw, ph);
    t.eq(&w("engine pixels"), [e.0, e.1, e.2, e.3], px);
    let e = engine.straightened_size(pw, ph);
    t.eq(&w("engine straightened_size"), [e.0, e.1], st);

    let (rr, rot, flip) = (fraction_rect(r), rotation(r), r.chance(2));
    t.dbg(&w("shown"), oracle::CropSettings::shown(o_rect(rr), rot, flip), o_rect(core::CropSettings::shown(rr, rot, flip)));
    t.dbg(&w("unrotated"), oracle::CropSettings::unrotated(o_rect(rr), rot, flip), o_rect(core::CropSettings::unrotated(rr, rot, flip)));
    t.dbg(&w("mirrored"), oracle::CropSettings::mirrored(o_rect(rr)), o_rect(core::CropSettings::mirrored(rr)));

    let (a, b, p) = (rect(r), rect(r), point(r));
    let (dx, dy) = (r.length() - 300.0, r.length() - 300.0);
    t.dbg(&w("inset"), o_rect(a).inset(dx, dy), o_rect(a.inset(dx, dy)));
    t.eq(&w("contains"), o_rect(a).contains(o_pt(p)), a.contains(p));
    let fp = core::Point { x: r.fraction(), y: r.fraction() };
    t.eq(&w("contains fraction"), o_rect(rr).contains(o_pt(fp)), rr.contains(fp));
    t.dbg(&w("intersection"), o_rect(a).intersection(&o_rect(b)), o_rect(a.intersection(&b)));
    t.dbg(&w("of_size"), oracle::Rect::of_size(o_size(sz)), o_rect(core::Rect::of_size(sz)));

    let o = orientation(r);
    let oo = o_orientation(o);
    let (turns, f) = (rotation(r), r.chance(2));
    let n = core::Orientation::new(turns, f);
    t.eq(&w("orientation new"), oracle::Orientation::new(turns, f), o_orientation(n));
    if o.turns.unsigned_abs() < 100 {
        t.eq(&w("flipped_as_shown"), oo.flipped_as_shown(), o_orientation(o.flipped_as_shown()));
        t.dbg(&w("orientation shown"), oo.shown(o_pt(fp)), o_pt(o.shown(fp)));
        t.dbg(&w("orientation unshown"), oo.unshown(o_pt(fp)), o_pt(o.unshown(fp)));
        let turned = size(r);
        let at = [r.fraction(), r.fraction()];
        t.dbg(&w("unshown_point"), oracle::unshown_point(oo, &os, o_size(turned), at), core::unshown_point(o, &s, turned, at).map(o_pt));
    }

    let bounds = rect(r);
    let ratio = match r.below(6) {
        0 => r.wild(),
        1 => 1.0,
        _ => r.range(0.2, 5.0),
    };
    let center = point(r);
    t.dbg(&w("fit"), oracle::fit(ratio, o_pt(center), o_rect(bounds)), o_rect(core::fit(ratio, center, bounds)));
    t.dbg(&w("move_rect"), oracle::move_rect(o_rect(a), dx, dy, o_rect(bounds)), o_rect(core::move_rect(a, dx, dy, bounds)));
    let handle = if r.chance(10) { core::Handle { x: r.int(-3, 3) as i32, y: r.int(-3, 3) as i32 } } else { core::Handle { x: r.int(-1, 1) as i32, y: r.int(-1, 1) as i32 } };
    let ohandle = oracle::Handle { x: handle.x, y: handle.y };
    let maybe_ratio = if r.chance(3) { None } else { Some(ratio) };
    let minimum = if r.chance(5) { r.wild() } else { r.range(0.0, 60.0) };
    t.dbg(
        &w("resize"),
        oracle::resize(o_rect(a), ohandle, o_pt(p), maybe_ratio, o_rect(bounds), minimum),
        o_rect(core::resize(a, handle, p, maybe_ratio, bounds, minimum)),
    );
    let frame = rect(r);
    let near = if r.chance(2) { core::Point { x: frame.x + r.range(-30.0, 30.0), y: frame.max_y() + r.range(-30.0, 30.0) } } else { p };
    let scale = if r.chance(4) { r.wild() } else { r.range(0.5, 3.0) };
    t.dbg(&w("handle_at"), oracle::handle_at(o_pt(near), o_rect(frame), scale), core::handle_at(near, frame, scale).map(|h| oracle::Handle { x: h.x, y: h.y }));
    let angle = r.angle();
    t.dbg(&w("turn"), oracle::turn(o_pt(p), angle, o_pt(center)), o_pt(core::turn(p, angle, center)));
    let photo = size(r);
    t.dbg(&w("corners"), oracle::corners(o_size(photo), angle), core::corners(photo, angle).map(o_pt));
    t.dbg(&w("bounds"), oracle::bounds(o_size(photo), angle), o_rect(core::bounds(photo, angle)));
    // A rectangle near the photograph, so fits and inscribe see both answers.
    let near_rect = core::Rect::new(
        photo.width * r.range(-0.2, 0.9),
        photo.height * r.range(-0.2, 0.9),
        photo.width * r.range(0.0, 1.2),
        photo.height * r.range(0.0, 1.2),
    );
    let target = core::Rect::new(near_rect.x + r.range(-500.0, 500.0), near_rect.y + r.range(-500.0, 500.0), near_rect.width * r.range(0.5, 1.5), near_rect.height * r.range(0.5, 1.5));
    for q in [near_rect, a] {
        t.eq(&w("fits"), oracle::fits(o_rect(q), angle, o_size(photo)), core::fits(q, angle, photo));
        t.dbg(&w("inscribe"), oracle::inscribe(o_rect(q), angle, o_size(photo)), o_rect(core::inscribe(q, angle, photo)));
    }
    let start = core::inscribe(near_rect, angle, photo);
    for (s0, s1) in [(start, target), (near_rect, target), (a, b)] {
        t.dbg(&w("constrain"), oracle::constrain(o_rect(s0), o_rect(s1), angle, o_size(photo)), o_rect(core::constrain(s0, s1, angle, photo)));
    }
}

/// The oracle's session and the core's geometry, side by side.
fn compare_session(t: &mut Tally, what: &dyn Fn() -> String, o: &oracle::CropSession, c: &core::SessionGeometry) {
    t.dbg(what, o.rect, o_rect(c.rect));
    t.eq(what, o.aspect, o_aspect(c.aspect));
    t.eq(what, o.portrait, c.portrait);
    t.dbg(what, o.custom, o_size(c.custom));
    t.f(what, o.angle, c.angle);
    t.dbg(what, o.photo, o_size(c.photo));
    t.eq(what, (o.rotation, o.flip, o.straightening), (c.rotation, c.flip, c.straightening));
    let os = o.settings();
    let cs = c.settings();
    t.dbg(what, os, o_settings(&cs));
    t.eq(what, o.modified(), c.modified());
    t.eq(what, o.orientation_portrait(), c.orientation_portrait());
    t.dbg(what, o.ratio(), c.ratio());
    t.dbg(what, o.shown(), o_rect(c.shown()));
    t.dbg(what, o.bounds(), o_rect(c.bounds()));
}

fn session(r: &mut Random, t: &mut Tally, i: usize) {
    let start = settings(r);
    let photo = if r.chance(10) { size(r) } else { core::Size::new(r.range(50.0, 7000.0), r.range(50.0, 7000.0)) };
    let (rot, flip) = (r.int(-5, 5) as i32, r.chance(2));
    let mut state = oracle_crate::state::EditState::default();
    state.crop = o_settings(&start);
    let mut o = oracle::CropSession::new(state, o_size(photo), rot, flip);
    let mut c = core::SessionGeometry::new(&start, photo, rot, flip);
    compare_session(t, &|| format!("session {i}: new"), &o, &c);
    for step in 0..12 {
        let op = r.below(12);
        match op {
            0 => {
                let a = aspect(r);
                o.choose(o_aspect(a));
                c.choose(a);
            }
            1 => {
                let p = r.chance(2);
                o.orient(p);
                c.orient(p);
            }
            2 => {
                let (wd, ht) = (r.chance(3).then(|| r.range(-5.0, 2000.0)), r.chance(3).then(|| r.range(-5.0, 2000.0)));
                o.set_custom(wd, ht);
                c.set_custom(wd, ht);
            }
            3 => {
                o.reset();
                c.reset();
            }
            4 => {
                o.begin_straightening();
                c.begin_straightening();
            }
            5 | 6 => {
                let d = if r.chance(5) { r.range(-100.0, 100.0) } else { r.range(-45.0, 45.0) };
                o.straighten(d);
                c.straighten(d);
            }
            7 => {
                o.end_straightening();
                c.end_straightening();
            }
            8 => {
                let (p, rot, flip) = (if r.chance(3) { photo.swapped() } else { photo }, r.int(-5, 5) as i32, r.chance(2));
                o.update(o_size(p), rot, flip);
                c.update(p, rot, flip);
            }
            9 => {
                let v = core::Rect::new(r.range(-100.0, photo.width), r.range(-100.0, photo.height), r.range(0.0, photo.width), r.range(0.0, photo.height));
                o.set_shown(o_rect(v));
                c.set_shown(v);
            }
            10 => {
                // A drag, as the canvas makes one.
                let shown = c.shown();
                let moved = core::constrain(shown, core::move_rect(shown, r.range(-200.0, 200.0), r.range(-200.0, 200.0), c.bounds()), c.angle, c.photo);
                o.set_shown(o_rect(moved));
                c.set_shown(moved);
            }
            _ => {
                let rr = fraction_rect(r);
                o.rect = o_rect(rr);
                c.rect = rr;
            }
        }
        compare_session(t, &|| format!("session {i} step {step} op {op}"), &o, &c);
    }
}

fn serde_format(r: &mut Random, t: &mut Tally, i: usize) {
    let w = |what: &'static str| move || format!("serde {i}: {what}");
    let s = settings(r);
    let os = o_settings(&s);
    let (a, b) = (serde_json::to_string(&os).unwrap(), serde_json::to_string(&s).unwrap());
    t.eq(&w("write"), a.clone(), b.clone());
    let back_o: Result<oracle::CropSettings, _> = serde_json::from_str(&a);
    let back_c: Result<core::CropSettings, _> = serde_json::from_str(&a);
    t.dbg(&w("read"), back_o.ok(), back_c.ok().map(|c| o_settings(&c)));
    // Older and odder files: fields left out, other types, other aspects.
    let parts = [
        format!(r#""rect":[[{},{}],[{},{}]]"#, s.rect.x, s.rect.y, s.rect.width, s.rect.height),
        format!(r#""aspect":"{}""#, ["original", "3:2", "custom", "16:9", "bogus", "square"][r.below(6) as usize]),
        format!(r#""custom":[{},{}]"#, s.custom.width, s.custom.height),
        format!(r#""angle":{}"#, if r.chance(4) { "\"x\"".to_string() } else { format!("{}", r.angle()) }),
        r#""extra":1"#.to_string(),
        r#""rect":[1,2]"#.to_string(),
    ];
    let chosen: Vec<&String> = parts.iter().filter(|_| r.chance(2)).collect();
    let text = format!("{{{}}}", chosen.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(","));
    let o: Result<oracle::CropSettings, _> = serde_json::from_str(&text);
    let c: Result<core::CropSettings, _> = serde_json::from_str(&text);
    t.eq(&w("odd read ok"), o.is_ok(), c.is_ok());
    if let (Ok(o), Ok(c)) = (o, c) {
        t.dbg(&w("odd read"), o, o_settings(&c));
        t.eq(&w("odd rewrite"), serde_json::to_string(&o).unwrap(), serde_json::to_string(&c).unwrap());
    }
    let p = core::Point { x: s.rect.x, y: s.rect.y };
    t.eq(&w("size"), serde_json::to_string(&oracle::Size::new(p.x, p.y)).unwrap(), serde_json::to_string(&core::Size::new(p.x, p.y)).unwrap());
}

#[test]
fn crop_matches_the_rust_and_the_engine() {
    let seed = seed(0x3b_c0de);
    let n = cases(10_000);
    gate("crop", seed, |t| {
        let mut r = Random(seed);
        for i in 0..n {
            geometry(&mut r, t, i);
            if i % 4 == 0 {
                session(&mut r, t, i);
            }
            serde_format(&mut r, t, i);
        }
        n
    });
}
