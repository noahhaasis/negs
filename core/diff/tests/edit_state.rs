//! The edit state (core/src/state) against the frozen windows/src/state.rs
//! (core/diff/oracle/state.rs), through the C ABI's handle and through the
//! negs-core-rules wrapper:
//! - serde both ways, byte for byte: random states written in declaration
//!   order (`to_string`), `json!` order (`to_value`), compact and pretty;
//!   random and mutated state texts (fields missing, null, of the wrong
//!   type, unknown, doubled; the array form; enum and colour forms; bytes
//!   changed) accepted or refused alike, and what is read written alike;
//! - PartialEq, rendered, match_inputs, restored_params, size_mode and
//!   size_target, rotation, flip, orientation, Roll::filtered and
//!   own_filtration on random states;
//! - all of it on the engine's real defaults (state_common), and the cost
//!   of a state crossing the ABI (printed, see `crossing_costs`).
//! Seeded (NEGS_RULES_SEEDS, default 10,000; NEGS_RULES_SEED); each test
//! checks the core leaked nothing.

mod rules_common;
mod state_common;

use negs_core_rules::json as core_json;
use negs_core_rules::state as wrapper;
use negs_core_sys::state as sys;
use negs_core_sys::take_buf;
use negs_state_oracle::state as oracle;
use rules_common::{Leaks, Random, Tally, cases, first_seed};
use serde_json::{Map, Value, json};
use state_common::*;

/// A state parsed by the core, freed on drop.
struct H(*mut sys::negs_state_t);

impl H {
    fn parse(text: &[u8]) -> Option<H> {
        let p = unsafe { sys::negs_state_parse(text.as_ptr(), text.len()) };
        (!p.is_null()).then_some(H(p))
    }
    fn of(s: &oracle::EditState) -> H {
        H::parse(serde_json::to_string(s).unwrap().as_bytes()).expect("the core reads serde's text")
    }
    fn write(&self, flags: u32) -> String {
        String::from_utf8(take_buf(unsafe { sys::negs_state_write(self.0, flags) })).unwrap()
    }
}

impl Drop for H {
    fn drop(&mut self) {
        unsafe { sys::negs_state_free(self.0) };
    }
}

fn to_wrapper(s: &oracle::EditState) -> wrapper::EditState {
    serde_json::from_str(&serde_json::to_string(s).unwrap()).unwrap()
}

fn st<T: ?Sized + serde::Serialize>(s: &T) -> String {
    serde_json::to_string(s).unwrap()
}

#[test]
fn serde_both_orders() {
    let _serial = serial();
    let leaks = Leaks::start();
    let mut tally = Tally::default();
    let d = engine_defaults();
    let n = cases();
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed ^ 0x57a7e);
        let s = edit_state(&mut r, &d);
        let text = st(&s);
        let value = serde_json::to_value(&s).unwrap();
        let h = H::of(&s);
        tally.check("to_string", seed, text.clone(), h.write(0));
        tally.check("to_value", seed, st(&value), h.write(sys::NEGS_JSON_SORTED));
        tally.check("to_string_pretty", seed, serde_json::to_string_pretty(&s).unwrap(), h.write(sys::NEGS_JSON_PRETTY));
        tally.check("to_value pretty", seed, serde_json::to_string_pretty(&value).unwrap(), h.write(sys::NEGS_JSON_SORTED | sys::NEGS_JSON_PRETTY));
        tally.check("wrapper serde", seed, text.clone(), st(&to_wrapper(&s)));
        // Read back from the sorted and the pretty text too.
        let sorted = H::parse(st(&value).as_bytes()).map(|h| h.write(0));
        tally.check("from to_value", seed, Some(text.clone()), sorted);
    }
    tally.finish("edit state serde", n);
    leaks.check("edit state serde");
}

const TOP: &[&str] = &["params", "film", "paper", "limitPreview", "border", "crop", "brightnessMatch", "roll"];
const BORDER: &[&str] = &["enabled", "style", "export", "instagram", "width", "filmWidth", "line", "color", "lineColor"];
const CROP: &[&str] = &["rect", "aspect", "custom", "angle"];
const MATCH: &[&str] = &["ev", "inputs", "highlightLimited"];
const ROLL: &[&str] = &["id", "magenta", "yellow", "paper"];

fn odd(r: &mut Random) -> Value {
    let all = [
        json!(null), json!(true), json!(0), json!(1), json!(-1), json!(1.5), json!(""), json!("x"), json!("#ffffff"), json!("#FFFFFF"), json!("#fff"),
        json!("#gggggg"), json!("ffffff"), json!("#0a0B0c"), json!("white"), json!("film"), json!("keyline"), json!("35mm"), json!("Film"),
        json!("original"), json!("16:9"), json!("custom"), json!("freeform"), json!([]), json!([1]), json!([1, 2]), json!([1, 2, 3]),
        json!([[0, 0], [1, 1]]), json!([[0.1, 0.2], [0.5, 0.5]]), json!([[0, 0], [1]]), json!([[0, 0], [1, 1], [2, 2]]), json!([["0", 0], [1, 1]]),
        json!({}), json!({"film": null}), json!({"film": 1}), json!({"white": null, "film": null}), json!({"square": null}), json!(u64::MAX),
        json!(-0.0), json!(1e300), json!({"id": "r", "magenta": 1, "yellow": 2}), json!({"ev": 1}), json!({"inputs": null}),
    ];
    if r.chance(5) { value(r, 3) } else { all[r.index(all.len())].clone() }
}

/// A struct's object with one field gone, changed, or added.
fn edit_fields(r: &mut Random, v: &mut Value, names: &[&str]) {
    let Value::Object(m) = v else { return };
    let name = if r.chance(5) { key(r) } else { names[r.index(names.len())].to_string() };
    match r.below(3) {
        0 => {
            m.remove(&name);
        }
        _ => {
            m.insert(name, odd(r));
        }
    }
}

/// The struct in the array form serde also reads: its fields' values in
/// declaration order, some left off the end or one too many.
fn as_seq(v: &Value, names: &[&str], r: &mut Random) -> Value {
    let mut items: Vec<Value> = names.iter().map(|n| v.get(*n).cloned().unwrap_or(Value::Null)).collect();
    match r.below(4) {
        0 => items.truncate(r.index(names.len() + 1)),
        1 => items.push(odd(r)),
        _ => {}
    }
    Value::Array(items)
}

const RAW: &[&str] = &[
    "\"\\ud800\"", "1e999", "-1e999", "\"\u{0}\"", "{\"\\x\":1}", "-", "01", "1.", "[1,]", "\"\\u12\"", "tru", "{\"a\" 1}", "1.5e+",
    "\"\\uDFFF\\uD800\"", "[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]",
    "18446744073709551616", "-9223372036854775809", "[\"\\/\", 0.0e0]",
];

/// A state's text, edited the ways serde's derives care about.
fn state_text(r: &mut Random, s: &oracle::EditState) -> Vec<u8> {
    let mut v = serde_json::to_value(s).unwrap();
    for _ in 0..r.below(3) {
        match r.below(6) {
            0 => edit_fields(r, &mut v, TOP),
            1 => edit_fields(r, &mut v["border"], BORDER),
            2 => edit_fields(r, &mut v["crop"], CROP),
            3 => edit_fields(r, &mut v["brightnessMatch"], MATCH),
            4 => edit_fields(r, &mut v["roll"], ROLL),
            _ => {
                if let Some(b) = v.get("border").cloned() {
                    v["border"] = as_seq(&b, BORDER, r);
                }
            }
        }
    }
    if r.chance(10) {
        v = as_seq(&v, TOP, r);
    }
    let mut text = if r.chance(4) { serde_json::to_vec_pretty(&v).unwrap() } else { serde_json::to_vec(&v).unwrap() };
    if r.chance(4) {
        // Raw text where a field may go: unknown fields serde skips without
        // reading (any nesting, any string bytes, any number size), or a
        // field twice.
        let opens: Vec<usize> = text.iter().enumerate().filter(|(_, c)| **c == b'{').map(|(i, _)| i + 1).collect();
        if let Some(&at) = opens.get(r.index(opens.len().max(1))) {
            let insert = match r.below(5) {
                0 => {
                    let n = 100 + r.index(200);
                    format!("\"zz\":{}0{},", "[".repeat(n), "]".repeat(n))
                }
                1 => format!("\"film\":\"x\","),
                2 => format!("\"{}\":1,", [TOP, BORDER, CROP, MATCH, ROLL][r.index(5)][0]),
                3 => "\"zz\":\"\u{ff}\\n\",".into(),
                _ => format!("\"zz\":{},", RAW[r.index(RAW.len())]),
            };
            let mut bytes = insert.into_bytes();
            if r.chance(6) {
                bytes = bytes.iter().map(|&c| if c == 0xc3 { 0xff } else { c }).collect();
            }
            text.splice(at..at, bytes);
        }
    }
    if r.chance(6) {
        text = mutate(r, &text);
    }
    text
}

#[test]
fn reads_as_serde() {
    let _serial = serial();
    let leaks = Leaks::start();
    let mut tally = Tally::default();
    let d = engine_defaults();
    let n = cases() * 3;
    let mut accepted = 0u64;
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed ^ 0x2ead);
        let s = edit_state(&mut r, &d);
        let text = state_text(&mut r, &s);
        let serde = serde_json::from_slice::<oracle::EditState>(&text).ok().map(|s| st(&s));
        accepted += serde.is_some() as u64;
        let core = H::parse(&text).map(|h| h.write(0));
        if serde.is_some() != core.is_some() && tally.first.len() < 12 {
            eprintln!("seed {seed}: {}", String::from_utf8_lossy(&text));
        }
        tally.check("from_slice", seed, serde, core);
    }
    eprintln!("edit state reads: {accepted} of {n} texts accepted");
    tally.finish("edit state reads", n);
    leaks.check("edit state reads");
}

/// `s` with a small change that may or may not matter to PartialEq.
fn tweak(r: &mut Random, s: &oracle::EditState) -> oracle::EditState {
    let mut t = s.clone();
    match r.below(10) {
        0 => {}
        1 => t.params = alter(r, &s.params, 20),
        2 => {
            // The same numbers in the other kind: 1 and 1.0 are the same.
            t.params = serde_json::from_str(&st(&s.params).replace(".0,", ",").replace(".0}", "}")).unwrap_or(s.params.clone())
        }
        3 => t.film = film(r),
        4 => t.limit_preview = !t.limit_preview,
        5 => t.border = border_settings(r),
        6 => t.crop = crop_settings(r),
        7 => {
            if let Some(m) = &mut t.brightness_match {
                match r.below(3) {
                    0 => m.ev = m.ev.map(|e| e + 1.0),
                    1 => m.inputs = m.inputs.as_ref().map(|i| alter(r, i, 10)).filter(|v| !v.is_null()),
                    _ => m.highlight_limited = !m.highlight_limited,
                }
            } else {
                t.brightness_match = Some(oracle::BrightnessMatch::default());
            }
        }
        8 => t.roll = if r.bool() { None } else { Some(roll(r, s)) },
        _ => t.paper = film(r),
    }
    t
}

#[test]
fn rules_as_state_rs() {
    let _serial = serial();
    let leaks = Leaks::start();
    let mut tally = Tally::default();
    let d = engine_defaults();
    let n = cases();
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed ^ 0x21e5);
        let s = edit_state(&mut r, &d);
        let t = tweak(&mut r, &s);
        let (hs, ht) = (H::of(&s), H::of(&t));
        let (ws, wt) = (to_wrapper(&s), to_wrapper(&t));
        tally.check("eq", seed, s == t, unsafe { sys::negs_state_eq(hs.0, ht.0) } == 1);
        tally.check("eq (wrapper)", seed, s == t, ws == wt);
        tally.check("eq itself", seed, true, ws == ws.clone());
        let rendered = H(unsafe { sys::negs_state_rendered(hs.0) });
        tally.check("rendered", seed, st(&s.rendered()), rendered.write(0));
        tally.check("rendered (wrapper)", seed, st(&s.rendered()), st(&ws.rendered()));
        tally.check("rendered eq", seed, s.rendered() == t.rendered(), ws.rendered() == wt.rendered());
        let inputs = String::from_utf8(take_buf(unsafe { sys::negs_state_match_inputs(hs.0) })).unwrap();
        tally.check("match_inputs", seed, st(&oracle::match_inputs(&s)), inputs);
        tally.check("match_inputs (wrapper)", seed, st(&oracle::match_inputs(&s)), st(&wrapper::match_inputs(&ws)));
        tally.check("rotation", seed, s.rotation(), unsafe { sys::negs_state_rotation(hs.0) });
        tally.check("rotation (wrapper)", seed, s.rotation(), ws.rotation());
        tally.check("flip", seed, s.flip(), unsafe { sys::negs_state_flip(hs.0) } == 1);
        tally.check("flip (wrapper)", seed, s.flip(), ws.flip());
        let mut o = sys::negs_orientation_t::default();
        unsafe { sys::negs_state_orientation(hs.0, &mut o) };
        let so = s.orientation();
        tally.check("orientation", seed, (so.turns, so.flip), (o.turns, o.flip == 1));
        let wo = ws.orientation();
        tally.check("orientation (wrapper)", seed, (so.turns, so.flip), (wo.turns, wo.flip));
        let target = oracle::size_target(&s.params);
        let mut out = 0u32;
        let has = unsafe { sys::negs_state_size_target(hs.0, &mut out) } == 1;
        tally.check("size_target", seed, target, has.then_some(out));
        let p = st(&s.params);
        let has = unsafe { sys::negs_state_params_size_target(p.as_ptr(), p.len(), &mut out) } == 1;
        tally.check("size_target (params)", seed, target, has.then_some(out));
        tally.check("size_target (wrapper)", seed, target, wrapper::size_target(&ws.params));
        tally.check("size_mode", seed, format!("{:?}", oracle::size_mode(&s.params)), format!("{:?}", wrapper::size_mode(&ws.params)));
        let roll = s.roll.clone().unwrap_or_else(|| roll(&mut r, &s));
        let wroll: wrapper::Roll = serde_json::from_str(&st(&roll)).unwrap();
        tally.check("Roll::filtered", seed, roll.filtered(&s), unsafe { sys::negs_state_roll_filtered(hs.0, roll.magenta, roll.yellow) } == 1);
        tally.check("Roll::filtered (wrapper)", seed, roll.filtered(&s), wroll.filtered(&ws));
        tally.check("Roll::own_filtration", seed, roll.own_filtration(&s), wroll.own_filtration(&ws));
        let (defaults, nullable) = if r.bool() { (d.defaults.clone(), d.nullable.clone()) } else { (value(&mut r, 4), vec![]) };
        let saved = if r.bool() { alter(&mut r, &defaults, 10) } else { s.params.clone() };
        tally.check("restored_params", seed, st(&oracle::restored_params(&defaults, &saved, &nullable)), st(&wrapper::restored_params(&defaults, &saved, &nullable)));
    }
    tally.finish("edit state rules", n);
    leaks.check("edit state rules");
}

/// A realistic state: the engine's defaults, a crop, a border, a roll.
fn realistic(d: &Defaults) -> oracle::EditState {
    let mut s = oracle::EditState { params: d.defaults.clone(), ..Default::default() };
    s.crop.rect = negs_state_oracle::crop::Rect::new(0.1, 0.05, 0.8, 0.85);
    s.crop.angle = 1.25;
    s.border.enabled = true;
    s.border.width = 0.05;
    s.roll = Some(oracle::Roll { id: "roll-1".into(), magenta: 2.0, yellow: -1.5, paper: Some(s.paper.clone()) });
    s
}

#[test]
fn engine_defaults_and_lossy_cases() {
    let _serial = serial();
    let leaks = Leaks::start();
    let mut tally = Tally::default();
    let d = engine_defaults();
    let s = realistic(&d);
    let w = to_wrapper(&s);
    let h = H::of(&s);
    tally.check("defaults to_string", 0, st(&s), h.write(0));
    tally.check("defaults to_value", 0, st(&serde_json::to_value(&s).unwrap()), h.write(sys::NEGS_JSON_SORTED));
    tally.check("defaults pretty", 0, serde_json::to_string_pretty(&s).unwrap(), h.write(sys::NEGS_JSON_PRETTY));
    tally.check("defaults rendered", 0, st(&s.rendered()), st(&w.rendered()));
    tally.check("defaults match_inputs", 0, st(&oracle::match_inputs(&s)), st(&wrapper::match_inputs(&w)));
    tally.check("defaults restored", 0, st(&oracle::restored_params(&d.defaults, &s.params, &d.nullable)), st(&wrapper::restored_params(&d.defaults, &s.params, &d.nullable)));
    let mut sized = s.clone();
    negs_state_oracle::json::set(&mut sized.params, oracle::SIZE_TARGET, json!(6000));
    tally.check("defaults sized rendered", 0, st(&sized.rendered()), st(&to_wrapper(&sized).rendered()));
    tally.check("defaults eq", 0, s == sized, w == to_wrapper(&sized));
    tally.check("Default", 0, st(&oracle::EditState::default()), st(&wrapper::EditState::default()));
    tally.check("constants", 0,
        [oracle::PREP_EXPOSURE, oracle::FLIP, oracle::SIZE_TARGET, oracle::SIZE_FACTOR, oracle::LENS_CORRECTION],
        [wrapper::PREP_EXPOSURE, wrapper::FLIP, wrapper::SIZE_TARGET, wrapper::SIZE_FACTOR, wrapper::LENS_CORRECTION]);

    // What JSON can't carry (serde's own limit, kept as it is): a NaN is
    // written as null, which serde refuses to read back into an f64, so
    // the core never sees such a state; equal to nothing, as NaN is.
    let mut nan = s.clone();
    nan.crop.angle = f64::NAN;
    assert!(serde_json::from_str::<oracle::EditState>(&st(&nan)).is_err());
    assert!(H::parse(st(&nan).as_bytes()).is_none());
    tally.check("NaN eq", 0, nan == nan.clone(), to_wrapper_lossy(&nan));
    // A match's `Some(null)` inputs are written as `"inputs":null`, read as
    // None: the oracle tells them apart, no JSON crossing can.
    let mut some_null = s.clone();
    some_null.brightness_match = Some(oracle::BrightnessMatch { ev: Some(0.5), inputs: Some(Value::Null), highlight_limited: false });
    let mut none = some_null.clone();
    none.brightness_match.as_mut().unwrap().inputs = None;
    assert_ne!(some_null, none);
    assert_eq!(st(&some_null), st(&none));
    tally.finish("edit state defaults", 1);
    drop(h);
    leaks.check("edit state defaults");
}

/// The wrapper's PartialEq on a state with a NaN in it, built field by
/// field (serde can't carry it).
fn to_wrapper_lossy(s: &oracle::EditState) -> bool {
    let mut w = wrapper::EditState { params: s.params.clone(), film: s.film.clone(), paper: s.paper.clone(), ..Default::default() };
    w.crop.angle = s.crop.angle;
    w == w.clone()
}

fn time<T>(label: &str, n: u32, mut f: impl FnMut() -> T) {
    let start = std::time::Instant::now();
    for _ in 0..n {
        std::hint::black_box(f());
    }
    eprintln!("  {label:<46} {:>8.2} µs", start.elapsed().as_secs_f64() * 1e6 / n as f64);
}

/// The cost of a state crossing the ABI, on a realistic state (the
/// engine's defaults, a crop, a border, a roll). Printed, not asserted:
/// `cargo test --release --test edit_state crossing_costs -- --nocapture`.
#[test]
fn crossing_costs() {
    let _serial = serial();
    let leaks = Leaks::start();
    let d = engine_defaults();
    let s = realistic(&d);
    let w = to_wrapper(&s);
    let w2 = w.clone();
    let text = st(&s);
    let n = 2000;
    eprintln!("crossing costs ({} bytes of state JSON, {} of params):", text.len(), st(&s.params).len());
    time("serde_json::to_string(&state) (Rust)", n, || st(&w));
    time("serde_json::from_str::<EditState> (Rust)", n, || serde_json::from_str::<wrapper::EditState>(&text).unwrap());
    time("negs_state_parse + free", n, || H::parse(text.as_bytes()));
    let h = H::of(&s);
    time("negs_state_write", n, || h.write(0));
    time("round trip: to_string, parse, write, from_str", n, || serde_json::from_str::<wrapper::EditState>(&H::parse(st(&w).as_bytes()).unwrap().write(0)).unwrap());
    eprintln!(" the wrapper (core) against the frozen Rust:");
    time("EditState == (wrapper)", n, || w == w2);
    time("EditState == (Rust)", n, || s == s.clone());
    time("rendered() (wrapper)", n, || w.rendered());
    time("rendered() (Rust)", n, || s.rendered());
    time("match_inputs (wrapper)", n, || wrapper::match_inputs(&w));
    time("match_inputs (Rust)", n, || oracle::match_inputs(&s));
    time("rotation() + flip() (wrapper)", n, || (w.rotation(), w.flip()));
    time("rotation() + flip() (Rust)", n, || (s.rotation(), s.flip()));
    time("size_target(params) (wrapper)", n, || wrapper::size_target(&w.params));
    time("Roll::filtered (wrapper)", n, || w.roll.as_ref().unwrap().filtered(&w));
    let mut p = w.params.clone();
    time("json::set(params, ..) (wrapper)", n, || core_json::set(&mut p, "prep.exposure_ev", json!(0.5)));
    let mut q = s.params.clone();
    time("json::set(params, ..) (Rust)", n, || negs_state_oracle::json::set(&mut q, "prep.exposure_ev", json!(0.5)));
    time("json::same(params, params) (wrapper)", n, || core_json::same(&w.params, &w2.params));
    time("json::number(leaf) (wrapper)", n, || core_json::number(&json!(1.5)));
    time("restored_params (wrapper)", 200, || wrapper::restored_params(&d.defaults, &w.params, &d.nullable));
    time("restored_params (Rust)", 200, || oracle::restored_params(&d.defaults, &s.params, &d.nullable));
    let _ = Map::<String, Value>::new();
    drop(h);
    leaks.check("edit state costs");
}
