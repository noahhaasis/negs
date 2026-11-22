//! Your negative's rules, Odin core (negs_core_rules::negative) against the
//! Rust oracle (windows/src/negative.rs as it was, core/diff/oracle).
//! Seeded inputs (NEGS_RULES_SEEDS, default 10,000; NEGS_RULES_SEED): the
//! scan stops and exposure, the skipped words, the route and the readable
//! stocks (through the edit state and stock the oracle takes), the found
//! paths, the picture's crop and bounds from the helper's JSON, the flat
//! field's row, the shown edge, the base's status and sentence, the picks'
//! prompts and a base's tolerance; every float bit for bit, every string
//! byte for byte.

mod picture_common;

use negs_core_diff_picture::{crop as ocrop, negative as oracle, state};
use negs_core_rules::{crop as ccrop, negative as core};
use picture_common::{Random, Tally, cases, gate, seed};
use serde_json::{Value, json};

fn light_value(r: &mut Random) -> Option<f64> {
    match r.below(8) {
        0 => None,
        1 => Some(r.wild()),
        2 => Some(0.0),
        3 => Some(-r.range(0.0, 10.0)),
        4 => Some([1.0 / 125.0, 1.0 / 60.0, 0.5, 2.0, 4.0, 5.6, 8.0, 100.0, 400.0][r.below(9) as usize]),
        _ => Some(r.range(1e-4, 3200.0)),
    }
}

fn light(r: &mut Random) -> core::ScanLight {
    (light_value(r), light_value(r), light_value(r))
}

const ROUTES: [&str; 6] = [core::ROUTE, core::SCENE_ROUTE, "", "input > convert-film > print > scan ", "INPUT > CONVERT-FILM > PRINT > SCAN", "input > convert-film"];

fn route_params(r: &mut Random) -> (Value, String) {
    let route = ROUTES[r.below(ROUTES.len() as u64) as usize];
    match r.below(6) {
        0 => (json!({}), String::new()),
        1 => (json!({"workflow": {"route": 3}}), String::new()),
        2 => (json!({"workflow": "route"}), String::new()),
        _ => (json!({"workflow": {"route": route}}), route.to_string()),
    }
}

/// A path near the found paths: one of them, a parent, a child, a sibling
/// that shares a prefix, or nonsense.
fn path(r: &mut Random) -> String {
    let all: Vec<&str> = core::FOUND_PATHS.iter().chain(&core::OWN_PATHS).chain(&core::PRESET_KEEPS).copied().collect();
    let p = all[r.below(all.len() as u64) as usize];
    match r.below(9) {
        0 => p.to_string(),
        1 => format!("{p}.r"),
        2 => format!("{p}x"),
        3 => p.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or_default(),
        4 => p.split('.').next().unwrap().to_string(),
        5 => format!("{p}."),
        6 => format!(".{p}"),
        7 => String::new(),
        _ => "film_render.base.é".into(),
    }
}

fn number(r: &mut Random) -> Value {
    match r.below(10) {
        0 => Value::Null,
        1 => json!("0.5"),
        2 => json!(true),
        3 => json!(r.int(-3, 3)),
        4 => json!(1u64 << 63),
        5 => json!(-1.0e-320),
        _ => json!(r.range(-0.2, 1.2)),
    }
}

fn picture(r: &mut Random) -> Value {
    if r.chance(10) {
        return [Value::Null, json!([0.1, 0.2, 0.8, 0.9]), json!("x"), json!(1)][r.below(4) as usize].clone();
    }
    let mut p = serde_json::Map::new();
    let (x0, y0) = (r.range(-0.05, 0.9), r.range(-0.05, 0.9));
    let (x1, y1) = (x0 + r.range(-0.05, 1.0 - x0 + 0.05), y0 + r.range(-0.05, 1.0 - y0 + 0.05));
    for (k, v) in [("x0", x0), ("y0", y0), ("x1", x1), ("y1", y1)] {
        if !r.chance(30) {
            p.insert(k.into(), if r.chance(15) { number(r) } else { json!(v) });
        }
    }
    match r.below(5) {
        0 => {}
        1 => {
            p.insert("angle_deg".into(), number(r));
        }
        _ => {
            p.insert("angle_deg".into(), json!(r.angle()));
        }
    }
    if r.chance(4) {
        p.insert("score".into(), json!(r.unit()));
    }
    Value::Object(p)
}

fn file_path(r: &mut Random) -> String {
    match r.below(9) {
        0 => String::new(),
        1 => "/Users/n/Light frames/light.dng".into(),
        2 => r"C:\Scans\Roll 12\flat.tif".into(),
        3 => "C:/Scans/mixed\\flat é.tif".into(),
        4 => "flat.tif".into(),
        5 => "/ends/with/".into(),
        6 => "\\".into(),
        7 => "東京/ライト.dng".into(),
        _ => format!("/scans/{}/frame {}.tif", r.below(100), r.below(40)),
    }
}

fn answer(r: &mut Random, path: &str) -> Value {
    let mut a = serde_json::Map::new();
    match r.below(5) {
        0 => {}
        1 => {
            a.insert("path".into(), json!("/other.tif"));
        }
        2 => {
            a.insert("path".into(), json!(3));
        }
        _ => {
            a.insert("path".into(), json!(path));
        }
    }
    match r.below(4) {
        0 => {}
        1 => {
            a.insert("used".into(), json!("yes"));
        }
        _ => {
            a.insert("used".into(), json!(r.chance(2)));
        }
    }
    match r.below(5) {
        0 => {}
        1 => {
            a.insert("reason".into(), json!(""));
        }
        2 => {
            a.insert("reason".into(), json!(false));
        }
        _ => {
            a.insert("reason".into(), json!(["The light frame is a different size.", "Lens differs", "é ✓"][r.below(3) as usize]));
        }
    }
    if r.chance(10) { json!([1]) } else { Value::Object(a) }
}

const EDGES: [&str; 7] = ["left", "right", "top", "bottom", "", "Left", "middle"];

fn orientation(r: &mut Random) -> (ocrop::Orientation, ccrop::Orientation) {
    let (turns, flip) = (r.int(-5, 8) as i32, r.chance(2));
    if r.chance(3) {
        (ocrop::Orientation { turns, flip }, ccrop::Orientation { turns, flip })
    } else {
        (ocrop::Orientation::new(turns, flip), ccrop::Orientation::new(turns, flip))
    }
}

fn base(r: &mut Random) -> Value {
    let mut b = serde_json::Map::new();
    for k in ["r", "g", "b", "density", "kind"] {
        if r.chance(5) {
            continue;
        }
        let v = match r.below(8) {
            0 => json!("auto"),
            1 => Value::Null,
            2 => json!(r.int(0, 3)),
            3 => json!([1, 2]),
            _ => json!(r.range(0.0, 2.0)),
        };
        b.insert(k.into(), v);
    }
    Value::Object(b)
}

/// `a` again, its numbers nudged by about the tolerance (1e-9 of the larger
/// of the value and 1) or more, or not at all; a key dropped or added now
/// and then.
fn nudged(r: &mut Random, a: &Value) -> Value {
    let Some(map) = a.as_object() else { return a.clone() };
    let mut b = map.clone();
    for v in b.values_mut() {
        if let Some(x) = v.as_f64() {
            let scale = x.abs().max(1.0) * 1e-9;
            let d = match r.below(6) {
                0 => 0.0,
                1 => scale,
                2 => -scale,
                3 => scale * r.range(0.9, 1.1),
                4 => f64::from_bits(x.to_bits() ^ 1) - x,
                _ => scale * r.range(-3.0, 3.0),
            };
            *v = json!(x + d);
        }
    }
    if r.chance(8) {
        let k = b.keys().next().cloned();
        if let Some(k) = k {
            b.remove(&k);
        }
    }
    if r.chance(8) {
        b.insert("extra".into(), json!(1));
    }
    Value::Object(b)
}

fn case(r: &mut Random, t: &mut Tally, i: usize) {
    let w = |what: &'static str| move || format!("case {i}: {what}");
    let (a, b) = (light(r), light(r));
    let (os, cs) = (oracle::scan_stops(a, b), core::scan_stops(a, b));
    t.dbg(&w("scan_stops"), os, cs);
    let roll = if r.chance(8) { r.wild() } else { r.range(-3.0, 3.0) };
    t.f(&w("exposure_ev"), oracle::exposure_ev(roll, os), core::exposure_ev(roll, cs));
    let n = match r.below(4) {
        0 => r.below(3) as usize,
        1 => usize::MAX - r.below(2) as usize,
        _ => r.below(10_000) as usize,
    };
    t.eq(&w("skipped"), oracle::skipped(n), core::skipped(n));

    let (params, route) = route_params(r);
    let st = state::EditState { params, ..Default::default() };
    t.eq(&w("is_negative"), oracle::is_negative(&st), core::is_route(&route));
    let (paper, positive, bw) = (r.chance(2), r.chance(2), r.chance(2));
    t.eq(&w("readable"), oracle::readable(&state::Stock { paper, positive, bw }), core::readable_kind(paper, positive, bw));
    let p = path(r);
    t.eq(&w("is_found"), oracle::is_found(&p), core::is_found(&p));

    let pic = picture(r);
    let (oc, cc) = (oracle::crop(&pic), core::crop(&pic));
    t.dbg(
        &w("crop"),
        oc,
        cc.map(|c| ocrop::CropSettings {
            rect: ocrop::Rect::new(c.rect.x, c.rect.y, c.rect.width, c.rect.height),
            aspect: ocrop::CropAspect::ALL[ccrop::CropAspect::ALL.iter().position(|a| *a == c.aspect).unwrap()],
            custom: ocrop::Size::new(c.custom.width, c.custom.height),
            angle: c.angle,
        }),
    );
    t.eq(&w("bounds"), oracle::bounds(&pic).map(|v| v.to_string()), core::bounds(&pic).map(|v| v.to_string()));

    let fp = file_path(r);
    let ans = answer(r, &fp);
    let (of, cf) = (oracle::FlatField::of(&fp, &ans), core::FlatField::of(&fp, &ans));
    t.eq(&w("flat field of"), (&of.path, of.used, &of.reason), (&cf.path, cf.used, &cf.reason));
    t.eq(&w("flat field is_on"), of.is_on(), cf.is_on());
    t.eq(&w("flat field value"), of.value(), cf.value());
    t.eq(&w("flat field hint"), of.hint(), cf.hint());
    // Every combination of the fields, as the app sets them.
    let used = [None, Some(false), Some(true)][r.below(3) as usize];
    let reason = [None, Some(""), Some("Lens differs")][r.below(3) as usize].map(String::from);
    let of = oracle::FlatField { path: fp.clone(), used, reason: reason.clone() };
    let cf = core::FlatField { path: fp, used, reason };
    t.eq(&w("flat field hint (set)"), of.hint(), cf.hint());
    t.eq(&w("flat field help"), oracle::FlatField::HELP, core::FlatField::HELP);

    let raw = r.chance(2);
    t.eq(&w("not_negative"), oracle::not_negative(raw), core::not_negative(raw));
    let (oo, co) = orientation(r);
    let edge = EDGES[r.below(EDGES.len() as u64) as usize];
    t.eq(&w("shown_edge"), oracle::shown_edge(oo, edge), core::shown_edge(co, edge));

    let mut result = serde_json::Map::new();
    for k in ["found", "picked"] {
        match r.below(4) {
            0 => {}
            1 => {
                result.insert(k.into(), json!(1));
            }
            _ => {
                result.insert(k.into(), json!(r.chance(2)));
            }
        }
    }
    match r.below(4) {
        0 => {}
        1 => {
            result.insert("edge".into(), json!(""));
        }
        _ => {
            result.insert("edge".into(), json!(edge));
        }
    }
    let result = Value::Object(result);
    let (mut ob, mut cb) = (oracle::BaseStatus::of(&result), core::BaseStatus::of(&result));
    let (by_hand, from_roll) = (r.chance(4), r.chance(4));
    ob.by_hand = by_hand;
    cb.by_hand = by_hand;
    ob.from_roll = from_roll;
    cb.from_roll = from_roll;
    t.eq(&w("base status"), (ob.found, ob.picked, &ob.edge, ob.by_hand, ob.from_roll), (cb.found, cb.picked, &cb.edge, cb.by_hand, cb.from_roll));
    t.eq(&w("guessed"), ob.guessed(), cb.guessed());
    t.eq(&w("sentence"), ob.sentence(oo), cb.sentence(co));

    t.eq(&w("pick base"), oracle::Pick::Base.prompt(), core::Pick::Base.prompt());
    t.eq(&w("pick neutral"), oracle::Pick::Neutral.prompt(), core::Pick::Neutral.prompt());

    let x = if r.chance(6) { [json!(1), json!("a"), Value::Null, json!([0.5])][r.below(4) as usize].clone() } else { base(r) };
    let y = if r.chance(3) { nudged(r, &x) } else { base(r) };
    t.eq(&w("same_base"), oracle::same_base(&x, &y), core::same_base(&x, &y));
    // The tolerance on its own, at the edge.
    let q = if r.chance(4) { r.wild() } else { r.range(-1e3, 1e3) };
    let d = q.abs().max(1.0) * 1e-9 * [1.0, -1.0, 1.0 + 1e-15, 1.0 - 1e-15, r.range(-2.0, 2.0)][r.below(5) as usize];
    let (p, q) = (json!({"v": q + d}), json!({"v": q}));
    t.eq(&w("same_base edge"), oracle::same_base(&p, &q), core::same_base(&p, &q));
}

#[test]
fn negative_matches_the_rust() {
    let seed = seed(0x3b_9e6);
    let n = cases(10_000);
    gate("negative", seed, |t| {
        let mut r = Random(seed);
        for i in 0..n {
            case(&mut r, t, i);
        }
        // The constants: kept as Rust consts, the core holds the same.
        t.eq(&|| "consts".into(), (oracle::ROUTE, oracle::SCENE_ROUTE, oracle::ROUTE_PATH, oracle::DEFAULT_FILM), (core::ROUTE, core::SCENE_ROUTE, core::ROUTE_PATH, core::DEFAULT_FILM));
        t.eq(&|| "consts".into(), (oracle::STOCK_ID, oracle::NAME, oracle::TILE_TEXT, oracle::SCAN_ILLUMINANT, oracle::FLAT_FIELD), (core::STOCK_ID, core::NAME, core::TILE_TEXT, core::SCAN_ILLUMINANT, core::FLAT_FIELD));
        t.eq(&|| "paths".into(), (oracle::OWN_PATHS, oracle::FOUND_PATHS, oracle::PRESET_KEEPS, oracle::SCAN_READING), (core::OWN_PATHS, core::FOUND_PATHS, core::PRESET_KEEPS, core::SCAN_READING));
        n
    });
}
