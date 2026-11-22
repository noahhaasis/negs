//! The tone graph, Odin core against the frozen Rust original
//! (core/diff/oracle/tone_graph.rs): seeded curves shaped like the helper's
//! (decreasing paper exposure, rising brightness; with plateaus, steps,
//! non-monotone stretches, NaN and infinities, short and mismatched
//! series), predicted for seeded controls; every handle, point, hit and
//! drag value on seeded geometries and pointers; the index helpers on
//! their own; and the JSON plumbing (controls, requests, matching, parsing),
//! which stays in Rust. Floats compare bit for bit; where the original
//! panics the wrapper must too. NEGS_RULES_SEEDS seeds (default 100, 100
//! curves each).

use negs_core_rules::tone_graph as core;
use negs_media_oracle::tone_graph as oracle;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
    fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn between(&mut self, low: f64, high: f64) -> f64 {
        low + self.unit() * (high - low)
    }
}

thread_local! {
    static QUIET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn run<T>(f: impl FnOnce() -> T) -> Result<T, ()> {
    QUIET.set(true);
    let result = catch_unwind(AssertUnwindSafe(f)).map_err(|_| ());
    QUIET.set(false);
    result
}

/// A float's bits, with every NaN as one value: Rust doesn't promise a NaN
/// result's sign or payload, and they differ by platform (x86's default NaN
/// is negative; a libm call such as UCRT's `pow` passes its operand's on).
trait CanonBits {
    fn canon_bits(self) -> u64;
}

impl CanonBits for f64 {
    fn canon_bits(self) -> u64 {
        if self.is_nan() { f64::NAN.to_bits() } else { self.to_bits() }
    }
}

fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|x| x.canon_bits()).collect()
}

fn bits3(v: &[Vec<f64>; 3]) -> [Vec<u64>; 3] {
    [bits(&v[0]), bits(&v[1]), bits(&v[2])]
}

type HandleBits = (u8, u64);

fn of_oracle_handle(h: oracle::Handle) -> HandleBits {
    match h {
        oracle::Handle::Middle => (0, 0),
        oracle::Handle::Shadows => (1, 0),
        oracle::Handle::Highlights => (2, 0),
        oracle::Handle::Curve(t) => (3, t.canon_bits()),
    }
}

fn of_core_handle(h: core::Handle) -> HandleBits {
    match h {
        core::Handle::Middle => (0, 0),
        core::Handle::Shadows => (1, 0),
        core::Handle::Highlights => (2, 0),
        core::Handle::Curve(t) => (3, t.canon_bits()),
    }
}

fn handle_pair(r: &mut Random) -> (oracle::Handle, core::Handle) {
    match r.below(4) {
        0 => (oracle::Handle::Middle, core::Handle::Middle),
        1 => (oracle::Handle::Shadows, core::Handle::Shadows),
        2 => (oracle::Handle::Highlights, core::Handle::Highlights),
        _ => {
            let t = special(r).unwrap_or_else(|| r.between(-7.0, 7.0));
            (oracle::Handle::Curve(t), core::Handle::Curve(t))
        }
    }
}

/// Now and then an awkward value.
fn special(r: &mut Random) -> Option<f64> {
    r.chance(40).then(|| [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -0.0, 1e300, -1e300, 5e-324][r.below(8) as usize])
}

fn spoil(r: &mut Random, v: &mut [f64]) {
    if !v.is_empty() && r.chance(30) {
        let i = r.below(v.len() as u64) as usize;
        v[i] = special(r).unwrap_or(f64::NAN);
    }
}

/// A series from `start` towards `end` over `n`, sometimes with plateaus,
/// steps back, or noise.
fn series(r: &mut Random, n: usize, start: f64, end: f64) -> Vec<f64> {
    let shape = r.below(6);
    let gamma = r.between(0.4, 2.5);
    let mut out: Vec<f64> = (0..n)
        .map(|i| {
            let u = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
            let u = match shape {
                0 => u.powf(gamma),
                1 => (u * 8.0).floor() / 8.0,
                2 => u + (r.unit() - 0.5) * 0.02,
                _ => u,
            };
            start + (end - start) * u
        })
        .collect();
    if r.chance(10) && n > 3 {
        let i = r.below(n as u64 - 1) as usize;
        out.swap(i, i + 1);
    }
    if r.chance(10) && n > 0 {
        // A flat run.
        let i = r.below(n as u64) as usize;
        let j = (i + r.below(20) as usize).min(n - 1);
        let v = out[i];
        out[i..=j].iter_mut().for_each(|x| *x = v);
    }
    spoil(r, &mut out);
    out
}

struct Data {
    stops: Vec<f64>,
    rgb: [Vec<f64>; 3],
    paper: Option<[Vec<f64>; 3]>,
    controls: (f64, f64, f64, f64),
}

fn data(r: &mut Random) -> Data {
    let n = match r.below(10) {
        0 => r.below(4) as usize,
        1 => 2 + r.below(10) as usize,
        _ => oracle::POINTS,
    };
    let (low, high) = (r.between(-9.0, -3.0), r.between(2.0, 9.0));
    let stops = series(r, n, low, high);
    let rgb = [0, 1, 2].map(|_| {
        let (a, b) = (r.between(-0.05, 0.1), r.between(0.85, 1.05));
        series(r, n, a, b)
    });
    let paper = (!r.chance(8)).then(|| {
        [0, 1, 2].map(|_| {
            // Mostly crossing 0 inside the range.
            let (a, b) = if r.chance(8) { (r.between(-3.0, 3.0), r.between(-3.0, 3.0)) } else { (r.between(0.2, 3.5), r.between(-3.5, -0.2)) };
            series(r, n, a, b)
        })
    });
    let control = |r: &mut Random| special(r).unwrap_or_else(|| if r.chance(3) { 1.0 } else { r.between(0.3, 3.0) });
    let controls = (control(r), control(r), control(r), control(r));
    Data { stops, rgb, paper, controls }
}

fn curves(d: &Data, request: &Value) -> (oracle::Curve, core::Curve) {
    let (pe, g, f, s) = d.controls;
    (
        oracle::Curve {
            request: request.clone(),
            controls: oracle::Controls { print_exposure: pe, gamma: g, fast: f, slow: s },
            stops: d.stops.clone(),
            paper: d.paper.clone(),
            rgb: d.rgb.clone(),
        },
        core::Curve {
            request: request.clone(),
            controls: core::Controls { print_exposure: pe, gamma: g, fast: f, slow: s },
            stops: d.stops.clone(),
            paper: d.paper.clone(),
            rgb: d.rgb.clone(),
        },
    )
}

fn next_controls(r: &mut Random, d: &Data) -> (oracle::Controls, core::Controls) {
    let (pe, g, f, s) = d.controls;
    let nudge = |r: &mut Random, v: f64| if r.chance(3) { v } else { special(r).unwrap_or_else(|| v * r.between(0.5, 2.0)) };
    let (pe, g, f, s) = (nudge(r, pe), nudge(r, g), nudge(r, f), nudge(r, s));
    (oracle::Controls { print_exposure: pe, gamma: g, fast: f, slow: s }, core::Controls { print_exposure: pe, gamma: g, fast: f, slow: s })
}

fn same_display(o: &oracle::Display, c: &core::Display, what: &str) {
    assert_eq!(bits(&c.stops), bits(&o.stops), "{what}: stops");
    assert_eq!(bits3(&c.rgb), bits3(&o.rgb), "{what}: rgb");
    assert_eq!(c.paper.as_ref().map(bits3), o.paper.as_ref().map(bits3), "{what}: paper");
}

#[derive(Default)]
struct Tally {
    curves: u64,
    predictions: u64,
    calls: u64,
    hits: u64,
    drags: u64,
    solved: u64,
    panics: u64,
}

fn channel(r: &mut Random) -> Option<usize> {
    match r.below(5) {
        0 => None,
        1 => Some(0),
        2 => Some(1),
        3 => Some(2),
        _ => None,
    }
}

fn geometry(r: &mut Random) -> (oracle::Geometry, core::Geometry) {
    let (w, h, i) = match r.below(10) {
        0 => (r.between(-5.0, 2.0), r.between(-5.0, 2.0), r.between(-2.0, 2.0)),
        1 => (special(r).unwrap_or(0.0), special(r).unwrap_or(0.0), special(r).unwrap_or(0.0)),
        _ => (r.between(150.0, 900.0), r.between(100.0, 600.0), r.between(0.0, 30.0)),
    };
    (oracle::Geometry { width: w, height: h, inset: i }, core::Geometry { width: w, height: h, inset: i })
}

fn point(r: &mut Random, g: &oracle::Geometry, near: Option<(f64, f64)>) -> (f64, f64) {
    if let Some((x, y)) = near.filter(|_| r.chance(2)) {
        return (x + r.between(-12.0, 12.0), y + r.between(-12.0, 12.0));
    }
    let x = special(r).unwrap_or_else(|| r.between(-50.0, g.width.abs() + 50.0));
    let y = special(r).unwrap_or_else(|| r.between(-50.0, g.height.abs() + 50.0));
    (x, y)
}

fn one_curve(r: &mut Random, t: &mut Tally) {
    let d = data(r);
    let request = json!({"film": "kodak_portra_400", "paper": "kodak_endura", "params": {"enlarger": {"print_exposure": d.controls.0}}});
    let (oc, cc) = curves(&d, &request);
    t.curves += 1;

    // The curve's own series.
    for ch in [None, Some(0), Some(1), Some(2)] {
        assert_eq!(run(|| bits(&cc.brightness(ch))), run(|| bits(&oc.brightness(ch))), "brightness {ch:?}");
        assert_eq!(run(|| cc.paper_exposure(ch).map(|v| bits(&v))), run(|| oc.paper_exposure(ch).map(|v| bits(&v))), "paper {ch:?}");
        t.calls += 2;
    }
    if r.chance(50) {
        let ch = Some(3 + r.below(3) as usize);
        let (a, b) = (run(|| bits(&cc.brightness(ch))), run(|| bits(&oc.brightness(ch))));
        assert_eq!(a.is_err(), b.is_err(), "brightness {ch:?} panics on both");
        t.panics += b.is_err() as u64;
    }

    // Predictions, and everything drawn on them.
    for _ in 0..3 {
        let (on, cn) = next_controls(r, &d);
        let od = oc.predicted(on);
        let cd = cc.predicted(cn);
        same_display(&od, &cd, &format!("predicted({on:?}) of {:?}", d.controls));
        t.predictions += 1;
        for _ in 0..4 {
            let ch = channel(r);
            let oh = od.handles(ch).map(|(a, b, c)| bits(&[a, b, c]));
            let ch_ = cd.handles(ch).map(|(a, b, c)| bits(&[a, b, c]));
            assert_eq!(ch_, oh, "handles {ch:?}");
            let stops = special(r).unwrap_or_else(|| r.between(-8.0, 8.0));
            let curve = od.brightness(ch);
            assert_eq!(bits(&cd.brightness(ch)), bits(&curve), "display brightness");
            assert_eq!(cd.value(&curve, stops).canon_bits(), od.value(&curve, stops).canon_bits(), "value at {stops}");
            let (og, cg) = geometry(r);
            for v in [stops, r.unit(), r.between(-2.0, 2.0), special(r).unwrap_or(0.5)] {
                assert_eq!(cg.x(v).canon_bits(), og.x(v).canon_bits(), "x({v})");
                assert_eq!(cg.stops(v * 300.0).canon_bits(), og.stops(v * 300.0).canon_bits(), "stops({v})");
                assert_eq!(cg.y(v).canon_bits(), og.y(v).canon_bits(), "y({v})");
                assert_eq!(cg.brightness(v * 300.0).canon_bits(), og.brightness(v * 300.0).canon_bits(), "brightness({v})");
            }
            let op: Vec<(HandleBits, u64, u64)> = og.handle_points(&od, ch).into_iter().map(|(h, (x, y))| (of_oracle_handle(h), x.canon_bits(), y.canon_bits())).collect();
            let cp: Vec<(HandleBits, u64, u64)> = cg.handle_points(&cd, ch).into_iter().map(|(h, (x, y))| (of_core_handle(h), x.canon_bits(), y.canon_bits())).collect();
            assert_eq!(cp, op, "handle points {ch:?} on {og:?}");
            t.calls += 12;
            let points = og.handle_points(&od, ch);
            for _ in 0..4 {
                let near = (!points.is_empty()).then(|| points[r.below(points.len() as u64) as usize].1);
                let p = point(r, &og, near);
                let (radius, tolerance) = (special(r).unwrap_or_else(|| r.between(0.0, 14.0)), r.between(0.0, 10.0));
                let a = og.hit(&od, ch, p, radius, tolerance).map(of_oracle_handle);
                let b = cg.hit(&cd, ch, p, radius, tolerance).map(of_core_handle);
                assert_eq!(b, a, "hit {p:?} r {radius} t {tolerance} {ch:?}");
                t.hits += 1;
            }
            // Drags, begun on the base curve with this display.
            let (oh, chh) = handle_pair(r);
            let os = oracle::Drag::start(oh, &oc, &od, ch);
            let cs = core::Drag::start(chh, &cc, &cd, ch);
            assert_eq!(cs.as_ref().map(|d| (of_core_handle(d.handle), d.tone.canon_bits())), os.as_ref().map(|d| (of_oracle_handle(d.handle), d.tone.canon_bits())), "drag start");
            if let (Some(os), Some(cs)) = (os, cs) {
                for _ in 0..4 {
                    let (on, cn) = next_controls(r, &d);
                    let p = point(r, &og, near_of(&points));
                    let dch = if r.chance(3) { channel(r) } else { ch };
                    let a = os.value(on, dch, &og, p).map(|(path, v)| (path, v.canon_bits()));
                    let b = cs.value(cn, dch, &cg, p).map(|(path, v)| (path, v.canon_bits()));
                    assert_eq!(b, a, "drag {:?} at {p:?} with {on:?} {dch:?} on {og:?}", os.handle);
                    t.drags += 1;
                    t.solved += a.is_some() as u64;
                }
            }
        }
    }
}

fn near_of(points: &[(oracle::Handle, (f64, f64))]) -> Option<(f64, f64)> {
    points.first().map(|p| p.1)
}

/// The index helpers on their own: sorted, reversed, flat, NaN, short.
fn helpers(r: &mut Random, t: &mut Tally) {
    let n = [0usize, 1, 2, 3, 5, 17, 256][r.below(7) as usize];
    let (a, b) = (r.between(-3.0, 3.0), r.between(-3.0, 3.0));
    let s = series(r, n, a, b);
    for _ in 0..8 {
        let v = special(r).unwrap_or_else(|| r.between(-4.0, 4.0));
        assert_eq!(core::index_of_decreasing(&s, v).canon_bits(), oracle::index_of_decreasing(&s, v).canon_bits(), "decreasing {s:?} {v}");
        assert_eq!(core::index_of_increasing(&s, v).canon_bits(), oracle::index_of_increasing(&s, v).canon_bits(), "increasing {s:?} {v}");
        let i = special(r).unwrap_or_else(|| r.between(-3.0, n as f64 + 3.0));
        assert_eq!(core::interpolate(&s, i).canon_bits(), oracle::interpolate(&s, i).canon_bits(), "interpolate {s:?} {i}");
        let (a, b, c) = (r.between(-1.0, 2.0), special(r).unwrap_or(r.unit()), r.unit());
        assert_eq!(core::Curve::luma(a, b, c).canon_bits(), oracle::Curve::luma(a, b, c).canon_bits());
        t.calls += 4;
    }
}

fn number(r: &mut Random) -> Value {
    match r.below(8) {
        0 => Value::Null,
        1 => json!(true),
        2 => json!("1.5"),
        3 => json!(0),
        4 => json!(-r.unit()),
        5 => json!(r.below(4)),
        _ => json!(r.between(0.0, 3.0)),
    }
}

/// The JSON side, in Rust on both: controls, requests, matching, parsing.
fn plumbing(r: &mut Random, t: &mut Tally) {
    let active = [json!(true), json!(false), Value::Null, json!(1)][r.below(4) as usize].clone();
    let (pe, g, f, s) = (number(r), number(r), number(r), number(r));
    let mut params = json!({"enlarger": {"print_exposure": pe}, "print_render": {"density_curves_morph": {
        "active": active, "gamma_factor": g, "gamma_factor_fast": f, "gamma_factor_slow": s}}});
    if r.chance(4) {
        params["film_render"] = json!({"grain": {"active": r.chance(2)}});
    }
    let lookup = |p: &Value, path: &str| -> Value { path.split('.').fold(p.clone(), |v, k| v.get(k).cloned().unwrap_or(Value::Null)) };
    let values: Vec<(String, Value)> = [oracle::PRINT_EXPOSURE, oracle::ACTIVE, oracle::GAMMA, oracle::FAST, oracle::SLOW].iter().map(|p| (p.to_string(), lookup(&params, p))).collect();
    let get = |path: &str| -> &Value { &values.iter().find(|(p, _)| p == path).unwrap().1 };
    let a = oracle::Controls::read(get);
    let b = core::Controls::read(get);
    assert_eq!(bits(&[b.print_exposure, b.gamma, b.fast, b.slow]), bits(&[a.print_exposure, a.gamma, a.fast, a.slow]), "{params}");
    let film = ["kodak_portra_400", "", "fuji"][r.below(3) as usize];
    let request = oracle::request(film, "kodak_endura", &params);
    assert_eq!(core::request(film, "kodak_endura", &params), request);
    // Another request: the same but for the controls, or not.
    let mut other = params.clone();
    match r.below(4) {
        0 => other["enlarger"]["print_exposure"] = json!(r.unit()),
        1 => other["print_render"]["density_curves_morph"]["gamma_factor"] = json!(2),
        2 => other["film_render"] = json!({"grain": {"active": true}}),
        _ => {}
    }
    let other = oracle::request(if r.chance(5) { "x" } else { film }, "kodak_endura", &other);
    // Curves parsed from helper-like replies, some broken.
    let n = [0usize, 1, 2, 5, 256][r.below(5) as usize];
    let triple = |r: &mut Random| -> Value {
        match r.below(30) {
            0 => json!([1, 2]),
            1 => json!([1, "x", 2]),
            2 => json!(null),
            _ => json!([r.between(-6.0, 6.0), r.between(-6.0, 6.0), r.between(-6.0, 6.0)]),
        }
    };
    let film_t: Vec<Value> = (0..n).map(|_| triple(r)).collect();
    let rgb_t: Vec<Value> = (0..n + (r.chance(10) as usize)).map(|_| triple(r)).collect();
    let paper_t: Vec<Value> = (0..n.saturating_sub(r.chance(8) as usize)).map(|_| triple(r)).collect();
    let mut reply = json!({"film": film_t, "rgb": rgb_t, "paper": paper_t});
    if r.chance(10) {
        reply["paper"] = Value::Null;
    }
    let oc = oracle::Curve::parse(&reply, request.clone(), a);
    let cc = core::Curve::parse(&reply, request.clone(), b);
    assert_eq!(cc.is_some(), oc.is_some(), "parse {reply}");
    if let (Some(oc), Some(cc)) = (oc, cc) {
        assert_eq!(bits(&cc.stops), bits(&oc.stops));
        assert_eq!(bits3(&cc.rgb), bits3(&oc.rgb));
        assert_eq!(cc.paper.as_ref().map(bits3), oc.paper.as_ref().map(bits3));
        assert_eq!(cc.matches(&other), oc.matches(&other), "matches {other}");
        assert!(cc.matches(&request) == oc.matches(&request));
    }
    // Film histograms.
    let bins = [0usize, 1, 2, 64][r.below(4) as usize];
    let layers = if r.chance(10) { 2 } else { 3 };
    let counts: Vec<Vec<Value>> = (0..layers).map(|l| (0..bins + (l == 1 && r.chance(10)) as usize).map(|_| json!(r.below(1000))).collect()).collect();
    let stops = match r.below(6) {
        0 => json!([1.0, -1.0]),
        1 => json!([0.0]),
        2 => json!([-6, "6"]),
        _ => json!([r.between(-8.0, -1.0), r.between(1.0, 8.0)]),
    };
    let histogram = json!({"stops": stops, "counts": counts});
    let oh = oracle::FilmHistogram::parse(&histogram);
    let ch = core::FilmHistogram::parse(&histogram);
    assert_eq!(ch.is_some(), oh.is_some(), "histogram {histogram}");
    if let (Some(oh), Some(chh)) = (oh, ch) {
        assert_eq!(bits(&[chh.range.0, chh.range.1]), bits(&[oh.range.0, oh.range.1]));
        for c in [None, Some(0), Some(1), Some(2)] {
            assert_eq!(bits(&chh.bins(c)), bits(&oh.bins(c)));
        }
    }
    t.calls += 6;
}

#[test]
fn the_core_draws_the_same_graph() {
    let before = negs_core_sys::alloc_stats();
    let hook = std::sync::Arc::new(std::panic::take_hook());
    let loud = hook.clone();
    std::panic::set_hook(Box::new(move |info| {
        if !QUIET.get() {
            loud(info)
        }
    }));
    let mut t = Tally::default();

    // The constants and handles' words.
    assert_eq!(
        [core::PRINT_EXPOSURE, core::ACTIVE, core::GAMMA, core::FAST, core::SLOW],
        [oracle::PRINT_EXPOSURE, oracle::ACTIVE, oracle::GAMMA, oracle::FAST, oracle::SLOW]
    );
    assert_eq!((core::EXPOSURE_RANGE, core::GAMMA_RANGE, core::SHOWN, core::POINTS), (oracle::EXPOSURE_RANGE, oracle::GAMMA_RANGE, oracle::SHOWN, oracle::POINTS));
    let mut r = Random(0x70e);
    for _ in 0..64 {
        let (a, b) = handle_pair(&mut r);
        let (c, d) = handle_pair(&mut r);
        assert_eq!((b.title(), b.path(), b.hint(), b.same(Some(d)), b.same(None)), (a.title(), a.path(), a.hint(), a.same(Some(c)), a.same(None)));
    }

    let seeds: u64 = std::env::var("NEGS_RULES_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(100);
    for seed in 0..seeds {
        let mut r = Random(seed ^ 0x7043);
        for _ in 0..100 {
            one_curve(&mut r, &mut t);
            helpers(&mut r, &mut t);
            plumbing(&mut r, &mut t);
        }
    }
    let _ = std::panic::take_hook();

    let after = negs_core_sys::alloc_stats();
    println!(
        "tone_graph: {} curves, {} predictions, {} hits, {} drags ({} solved), {} other calls, {} where both panic: 0 differences; core live allocations {} -> {} ({} made), bad frees {}",
        t.curves,
        t.predictions,
        t.hits,
        t.drags,
        t.solved,
        t.calls,
        t.panics,
        before.live_count,
        after.live_count,
        after.total_count - before.total_count,
        after.bad_frees
    );
    if after.tracked == 1 {
        assert_eq!(after.live_count, before.live_count, "the core leaked");
        assert_eq!(after.bad_frees, 0);
    }
}
