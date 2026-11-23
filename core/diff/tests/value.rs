//! The serde_json-faithful value (core/src/value) against serde_json
//! itself (1.0.154, float_roundtrip) and the frozen windows/src/json.rs:
//! - writing: random Values, compact and pretty, byte for byte;
//! - reading: random and mutated texts (numbers from the grammar's pieces,
//!   long mantissas, huge exponents, nests at the recursion limit, bad
//!   UTF-8, escapes and surrogates), accepted or refused alike, and what
//!   is accepted read to the same tree and number kinds (its compact text);
//! - doubles: serde's text and Rust's `{}` on at least a million doubles;
//! - every json.rs helper on random values and paths, through the
//!   negs-core-rules wrapper.
//! Seeded (NEGS_RULES_SEEDS, default 10,000; NEGS_RULES_SEED); each test
//! checks the core leaked nothing.

mod rules_common;
mod state_common;

use negs_core_rules::json as core;
use negs_core_sys::state as sys;
use negs_core_sys::take_buf;
use negs_state_oracle::json as oracle;
use rules_common::{Leaks, Random, Tally, cases, first_seed};
use serde_json::Value;
use state_common::*;

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn writes_as_serde() {
    let _serial = serial();
    let leaks = Leaks::start();
    let mut tally = Tally::default();
    let n = cases();
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed);
        let v = value(&mut r, 6);
        let compact = serde_json::to_string(&v).unwrap();
        let pretty = serde_json::to_string_pretty(&v).unwrap();
        tally.check("compact", seed, Some(compact.clone()), rewrite(compact.as_bytes(), 0).map(|b| s(&b)));
        tally.check("pretty", seed, Some(pretty.clone()), rewrite(compact.as_bytes(), sys::NEGS_JSON_PRETTY).map(|b| s(&b)));
        tally.check("pretty read back", seed, Some(compact), rewrite(pretty.as_bytes(), 0).map(|b| s(&b)));
    }
    tally.finish("value writes", n);
    leaks.check("value writes");
}

#[test]
fn reads_as_serde() {
    let _serial = serial();
    let leaks = Leaks::start();
    let mut tally = Tally::default();
    let n = cases() * 5;
    let mut accepted = 0;
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed ^ 0x5eed);
        let t = json_text(&mut r);
        let serde = serde_json::from_slice::<Value>(&t).ok().map(|v| serde_json::to_string(&v).unwrap());
        accepted += serde.is_some() as u64;
        let core = rewrite(&t, 0).map(|b| s(&b));
        if serde != core && tally.first.len() < 12 {
            eprintln!("text: {:?}", s(&t));
        }
        tally.check("from_slice", seed, serde, core);
    }
    eprintln!("value reads: {accepted} of {n} texts accepted");
    tally.finish("value reads", n);
    leaks.check("value reads");
}

#[test]
fn fixed_texts() {
    let _serial = serial();
    let leaks = Leaks::start();
    let mut tally = Tally::default();
    let texts: &[&[u8]] = &[
        b"", b" ", b"0", b"-0", b"-0.0", b"01", b"1.", b".1", b"1e", b"1e+", b"-", b"+1", b"[1,]", b"{\"a\":1,}", b"{1:2}", b"nul", b"1 2",
        b"18446744073709551615", b"18446744073709551616", b"-9223372036854775808", b"-9223372036854775809", b"1e309", b"-1e309",
        b"1.7976931348623157e308", b"1.7976931348623158e308", b"1.7976931348623159e308", b"4.9e-324", b"2.4e-324", b"2.5e-324", b"1e-400",
        b"0e99999999999", b"1e-99999999999", b"1e99999999999", b"0.000000000000000000000000000000000000000000001e45",
        b"2.2250738585072011e-308", b"2.2250738585072012e-308", b"9007199254740993", b"9007199254740993.0", b"[1e16,1e15,1e-5,1e-6,123456789012345678]",
        b"\"\\ud800\"", b"\"\\udc00\"", b"\"\\ud800\\udc00\"", b"\"\\ud800\\u0041\"", b"\"\\uD83D\\uDE00\"", b"\"\\u00e9\\/\\b\\f\"", b"\"\x7f\"",
        b"\"\x1f\"", b"\"\xff\"", b"\"\xed\xa0\x80\"", b"\"\xf4\x90\x80\x80\"", b"\"\xc0\x80\"", b"{\"a\":1,\"a\":2}", b"{\"b\":1,\"a\":2,\"\":3}",
        b"\x0c1", b"1\x0c", b"[\t1\r\n]", b"true ", b"[true,false,null]", b"{\"a\" : [ ] , \"b\" : { } }",
    ];
    for (i, t) in texts.iter().enumerate() {
        let serde = serde_json::from_slice::<Value>(t).ok().map(|v| serde_json::to_string(&v).unwrap());
        tally.check("fixed text", i as u64, serde, rewrite(t, 0).map(|b| s(&b)));
    }
    for n in [126, 127, 128, 129] {
        for open in ["[", "{\"k\":"] {
            let close = if open == "[" { "]" } else { "}" };
            let t = format!("{}0{}", open.repeat(n), close.repeat(n));
            let serde = serde_json::from_str::<Value>(&t).ok().map(|v| serde_json::to_string(&v).unwrap());
            tally.check("nest", n as u64, serde, rewrite(t.as_bytes(), 0).map(|b| s(&b)));
        }
    }
    tally.finish("value fixed texts", texts.len() as u64);
    leaks.check("value fixed texts");
}

fn display(x: f64) -> String {
    let mut out = [0u8; 400];
    let n = unsafe { sys::negs_value_display_f64(x, out.as_mut_ptr(), out.len()) };
    s(&out[..n])
}

fn written(x: f64) -> String {
    let mut out = [0u8; 64];
    let n = unsafe { sys::negs_value_write_f64(x, out.as_mut_ptr(), out.len()) };
    s(&out[..n])
}

#[test]
fn doubles_as_serde_and_display() {
    let _serial = serial();
    let leaks = Leaks::start();
    let mut tally = Tally::default();
    let n = (cases() * 100).max(1_000_000);
    let edges = [
        0.0, -0.0, f64::NAN, -f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, f64::MIN, f64::MIN_POSITIVE, f64::EPSILON, 5e-324, 1e-5, 1e-6,
        1e15, 1e16, 1e17, 1e21, 1e22, 1e23, 0.1, 0.3, 1.0 / 3.0, 9007199254740993.0, 123456789.0, 2.5, 1e300, 1e-300,
    ];
    for (i, &x) in edges.iter().enumerate() {
        tally.check("display edge", i as u64, format!("{x}"), display(x));
        tally.check("serde edge", i as u64, serde_json::to_string(&x).unwrap(), written(x));
    }
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed ^ 0xd0b1e);
        let x = if r.chance(4) { f64::from_bits(r.next()) } else { double(&mut r) };
        tally.check("display", seed, format!("{x}"), display(x));
        tally.check("serde", seed, serde_json::to_string(&x).unwrap(), written(x));
    }
    tally.finish("value doubles", n);
    leaks.check("value doubles");
}

fn core_get(v: &Value, path: &str) -> String {
    let t = text(v);
    let mut out = empty();
    let ok = unsafe { sys::negs_json_get(t.as_ptr(), t.len(), path.as_ptr(), path.len(), &mut out) };
    assert_eq!(ok, 1);
    s(&take_buf(out))
}

fn core_string(v: &Value) -> String {
    let t = text(v);
    let mut out = empty();
    let ok = unsafe { sys::negs_json_string(t.as_ptr(), t.len(), &mut out) };
    assert_eq!(ok, 1);
    s(&take_buf(out))
}

fn nullable_paths(r: &mut Random, v: &Value) -> Vec<String> {
    let all = paths(r, v);
    let mut out: Vec<String> = (0..r.index(6)).map(|_| all[r.index(all.len())].clone()).collect();
    for p in out.iter_mut() {
        if r.chance(3) {
            // `a.0` as restoring names an element: `a[*]`.
            if let Some((head, tail)) = p.rsplit_once('.') {
                if tail.parse::<usize>().is_ok() {
                    *p = format!("{head}[*]");
                }
            }
        }
    }
    out
}

#[test]
fn helpers_as_json_rs() {
    let _serial = serial();
    let leaks = Leaks::start();
    let mut tally = Tally::default();
    let n = cases();
    let d = engine_defaults();
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed ^ 0x4e1);
        let v = if r.chance(4) { alter(&mut r, &d.defaults, 30) } else { value(&mut r, 5) };
        let all = paths(&mut r, &v);
        for _ in 0..4 {
            let path = &all[r.index(all.len())];
            let got = oracle::get(&v, path);
            tally.check("get", seed, text(got), core_get(&v, path));
            tally.check("get (wrapper)", seed, text(got), text(core::get(&v, path)));
            tally.check("number", seed, oracle::number(got).to_bits(), core::number(got).to_bits());
            tally.check("boolean", seed, oracle::boolean(got), core::boolean(got));
            tally.check("string", seed, oracle::string(got).to_string(), core_string(got));
            let new = if r.bool() { scalar(&mut r) } else { value(&mut r, 2) };
            let (mut a, mut b) = (v.clone(), v.clone());
            oracle::set(&mut a, path, new.clone());
            core::set(&mut b, path, new);
            tally.check("set", seed, text(&a), text(&b));
        }
        let x = if r.chance(3) { f64::from_bits(r.next()) } else { double(&mut r) };
        let (o, c) = (oracle::num(x), core::num(x));
        tally.check("num", seed, (text(&o), o.is_f64()), (text(&c), c.is_f64()));
        let w = alter(&mut r, &v, 8);
        tally.check("same", seed, oracle::same(&v, &w), core::same(&v, &w));
        tally.check("same (itself)", seed, oracle::same(&v, &v), core::same(&v, &v));
        let (defaults, saved) = if r.bool() { (v.clone(), alter(&mut r, &v, 6)) } else { (alter(&mut r, &v, 6), v.clone()) };
        let nullable = nullable_paths(&mut r, &defaults);
        tally.check("restoring", seed, text(&oracle::restoring(&defaults, &saved, &nullable)), text(&core::restoring(&defaults, &saved, &nullable)));
    }
    // The engine's own defaults, with the catalog's nullable paths.
    for seed in 0..200 {
        let mut r = Random(seed);
        let saved = alter(&mut r, &d.defaults, 15);
        tally.check("restoring defaults", seed, text(&oracle::restoring(&d.defaults, &saved, &d.nullable)), text(&core::restoring(&d.defaults, &saved, &d.nullable)));
    }
    tally.finish("json helpers", n);
    leaks.check("json helpers");
}
