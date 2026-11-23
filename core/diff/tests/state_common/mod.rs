//! What Phase 4's differential tests share: random serde_json Values and
//! JSON texts, random edit states (the oracle's types), the core's value
//! and state calls on bytes, and the real engine defaults.
//!
//! `engine_defaults.json` is `{"defaults", "nullable_paths"}` from the
//! catalog `spektrafilm-native` (b05e49e) replies with, as the app
//! restores sessions on it. Built on rules_common's Random, Tally and
//! Leaks (NEGS_RULES_SEEDS, NEGS_RULES_SEED).

#![allow(dead_code)]

use crate::rules_common::Random;
use negs_core_sys::state as sys;
use negs_core_sys::{negs_buf_t, take_buf};
use negs_state_oracle::{border, crop, state};
use serde_json::{Map, Number, Value};

/// One test of a file at a time, so each one's leak check counts only its
/// own allocations (the core's counts are the process's).
pub fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn empty() -> negs_buf_t {
    negs_buf_t { data: std::ptr::null_mut(), len: 0 }
}

/// The core's rewrite of `text`: None where it refuses it.
pub fn rewrite(text: &[u8], flags: u32) -> Option<Vec<u8>> {
    let mut out = empty();
    let ok = unsafe { sys::negs_value_rewrite(text.as_ptr(), text.len(), flags, &mut out) };
    let bytes = take_buf(out);
    (ok == 1).then_some(bytes)
}

pub fn text(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}

// MARK: Values

/// A double at the edges the writers and readers care about.
pub fn double(r: &mut Random) -> f64 {
    let x = match r.below(16) {
        0 => 0.0,
        1 => f64::from_bits(r.below(1 << 52)),
        2 => 10f64.powi(r.below(640) as i32 - 330),
        3 => {
            let t: [f64; 9] = [1e-5, 1e-6, 1e15, 1e16, 1e17, 1e21, 1e-7, 0.1, 1.0];
            let x = t[r.index(t.len())];
            match r.below(3) {
                0 => x,
                1 => f64::from_bits(x.to_bits() + 1),
                _ => f64::from_bits(x.to_bits() - 1),
            }
        }
        4 => f64::MAX,
        5 => f64::MIN_POSITIVE,
        6 => r.below(1 << 54) as f64,
        7 => r.below(1_000_000) as f64 / 1000.0,
        8 => r.below(10_000) as f64 / 100.0,
        9 => r.unit(),
        10 | 11 => {
            let x = f64::from_bits(r.next());
            if x.is_finite() { x } else { 1.5 }
        }
        12 => 9.0e15 + r.below(10) as f64 - 5.0,
        13 => (r.below(1 << 20) as f64) * 2f64.powi(r.below(200) as i32 - 100),
        _ => r.between(-100.0, 100.0),
    };
    let x = if x.is_finite() { x } else { 1.5 };
    if r.bool() { -x } else { x }
}

pub fn number(r: &mut Random) -> Value {
    match r.below(12) {
        0 => Value::from(r.next()),
        1 => Value::from(r.below(1000)),
        2 => Value::from(-((r.next() >> 1) as i64) - 1),
        3 => Value::from([0u64, u64::MAX, i64::MAX as u64, 1 << 53][r.index(4)]),
        4 => Value::from([i64::MIN, -1, -(1 << 53)][r.index(3)]),
        5 => Value::from(-(r.below(1000) as i64)),
        _ => Value::Number(Number::from_f64(double(r)).unwrap()),
    }
}

const CHARS: &[char] = &[
    'a', 'b', 'z', 'A', '0', '9', ' ', '_', '.', '[', ']', '*', '"', '\\', '/', '\u{0}', '\u{1}', '\n', '\t', '\r', '\u{8}', '\u{c}', '\u{1f}',
    '\u{7f}', 'é', 'ß', '\u{2028}', '\u{301}', '€', '中', '\u{ffff}', '😀', '\u{10ffff}',
];

pub fn string(r: &mut Random) -> String {
    let n = match r.below(5) {
        0 => 0,
        1 => 1 + r.index(40),
        _ => 1 + r.index(8),
    };
    (0..n).map(|_| CHARS[r.index(CHARS.len())]).collect()
}

const KEYS: &[&str] = &["a", "b", "io", "rotate_cw", "flip", "0", "1", "", "film", "params", "aa", "B", "é", "a.b"];

pub fn key(r: &mut Random) -> String {
    if r.chance(3) { string(r) } else { KEYS[r.index(KEYS.len())].to_string() }
}

pub fn scalar(r: &mut Random) -> Value {
    match r.below(8) {
        0 => Value::Null,
        1 => Value::Bool(r.bool()),
        2 => Value::String(string(r)),
        _ => number(r),
    }
}

pub fn value(r: &mut Random, depth: u32) -> Value {
    if depth == 0 || r.chance(3) {
        return scalar(r);
    }
    let n = r.index(6);
    if r.bool() {
        Value::Array((0..n).map(|_| value(r, depth - 1)).collect())
    } else {
        let mut map = Map::new();
        for _ in 0..n {
            map.insert(key(r), value(r, depth - 1));
        }
        Value::Object(map)
    }
}

/// A number's text from the grammar's pieces, valid or nearly.
pub fn number_text(r: &mut Random) -> String {
    let mut s = String::new();
    match r.below(10) {
        0 => s.push('+'),
        1..=4 => s.push('-'),
        _ => {}
    }
    let digits = |r: &mut Random, n: usize| -> String { (0..n).map(|_| (b'0' + r.below(10) as u8) as char).collect() };
    match r.below(8) {
        0 => s.push('0'),
        1 => s += "00",
        2 => {
            let n = 1 + r.index(3);
            s += &format!("0{}", digits(r, n));
        }
        3 => {}
        4 => {
            s.push((b'1' + r.below(9) as u8) as char);
            let n = 17 + r.index(800);
            s += &digits(r, n);
        }
        _ => {
            s.push((b'1' + r.below(9) as u8) as char);
            let n = r.index(25);
            s += &digits(r, n);
        }
    }
    match r.below(5) {
        0 => s.push('.'),
        1 | 2 => {
            s.push('.');
            let n = if r.chance(4) { 17 + r.index(800) } else { 1 + r.index(20) };
            s += &digits(r, n);
        }
        _ => {}
    }
    if r.chance(2) {
        s.push(if r.bool() { 'e' } else { 'E' });
        match r.below(3) {
            0 => s.push('+'),
            1 => s.push('-'),
            _ => {}
        }
        match r.below(8) {
            0 => {}
            1 => s += &["2147483647", "2147483648", "99999999999999999999", "4294967296"][r.index(4)],
            2 => s += &format!("{}", 300 + r.below(30)),
            _ => {
                let n = 1 + r.index(3);
                s += &digits(r, n);
            }
        }
    }
    s
}

/// `[`/`{` nested about as deep as serde's limit, sometimes left open.
pub fn deep(r: &mut Random) -> String {
    let n = 120 + r.index(16);
    let opens: Vec<bool> = (0..n).map(|_| r.bool()).collect();
    let mut s = String::new();
    for &array in &opens {
        s += if array { "[" } else { "{\"k\":" };
    }
    s += "1";
    let close = if r.chance(5) { n - 1 } else { n };
    for &array in opens.iter().rev().take(close) {
        s.push(if array { ']' } else { '}' });
    }
    s
}

const PIECES: &[&[u8]] = &[
    b"[", b"]", b"{", b"}", b"\"", b",", b":", b" ", b"\n", b"0", b"-", b".", b"e", b"+", b"\\", b"\\u", b"\\ud800", b"\\udc00", b"\\ud83d\\ude00",
    b"null", b"true", b"fals", b"\x00", b"\x1f", b"\xff", b"\xc3", b"\xe2\x80", b"\xed\xa0\x80", b"\xf4\x90\x80\x80", b"\xc3\xa9", b"\t", b"\r", b"/",
    b"\\x", b"\\/", b"1e999", b"NaN", b"Infinity",
];

/// `text` with a few random edits: bytes removed, doubled, replaced, or
/// JSON's own pieces put in.
pub fn mutate(r: &mut Random, text: &[u8]) -> Vec<u8> {
    let mut t = text.to_vec();
    for _ in 0..1 + r.index(3) {
        let at = r.index(t.len() + 1);
        match r.below(5) {
            0 if !t.is_empty() => {
                let at = at.min(t.len() - 1);
                t.remove(at);
            }
            1 if !t.is_empty() => {
                let at = at.min(t.len() - 1);
                t[at] = r.below(256) as u8;
            }
            2 if at < t.len() => {
                let end = (at + 1 + r.index(8)).min(t.len());
                let copy = t[at..end].to_vec();
                t.splice(at..at, copy);
            }
            _ => {
                let p = PIECES[r.index(PIECES.len())];
                t.splice(at..at, p.iter().copied());
            }
        }
    }
    t
}

/// A JSON text: from a random value (compact or pretty), a number, a deep
/// nest or a string of escapes, then often mutated.
pub fn json_text(r: &mut Random) -> Vec<u8> {
    let base: Vec<u8> = match r.below(8) {
        0 => number_text(r).into_bytes(),
        1 => format!("[{}]", number_text(r)).into_bytes(),
        2 => deep(r).into_bytes(),
        3 => serde_json::to_vec_pretty(&value(r, 4)).unwrap(),
        _ => serde_json::to_vec(&value(r, 4)).unwrap(),
    };
    let mut t = if r.chance(3) { base } else { mutate(r, &base) };
    if r.chance(6) {
        let lead: &[u8] = [b" ".as_slice(), b"\n\t\r", b"\x0c"][r.index(3)];
        t.splice(0..0, lead.iter().copied());
    }
    if r.chance(6) {
        t.extend_from_slice([b" ".as_slice(), b"\n", b" x", b"0"][r.index(4)]);
    }
    t
}

/// The paths into `v` (dotted, as json::get reads them), and some that miss.
pub fn paths(r: &mut Random, v: &Value) -> Vec<String> {
    fn walk(v: &Value, prefix: &str, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, c) in m {
                    let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                    out.push(p.clone());
                    walk(c, &p, out);
                }
            }
            Value::Array(a) => {
                for (i, c) in a.iter().enumerate() {
                    let p = if prefix.is_empty() { i.to_string() } else { format!("{prefix}.{i}") };
                    out.push(p.clone());
                    walk(c, &p, out);
                }
            }
            _ => {}
        }
    }
    let mut out = vec![String::new()];
    walk(v, "", &mut out);
    for _ in 0..3 {
        let base = out[r.index(out.len())].clone();
        let tail = ["missing", "0", "01", "+1", "-1", "", "18446744073709551616", "1 ", "x"][r.index(9)];
        out.push(if base.is_empty() { tail.into() } else { format!("{base}.{tail}") });
    }
    out
}

/// `v` changed here and there: numbers to the other kind (1 and 1.0),
/// values swapped, keys dropped or added, nulls. For json::same, the
/// PartialEqs and restoring's saved values.
pub fn alter(r: &mut Random, v: &Value, rate: u64) -> Value {
    if r.chance(rate) {
        return match r.below(4) {
            0 => Value::Null,
            1 => scalar(r),
            2 => value(r, 2),
            _ => match v.as_f64() {
                Some(x) if v.is_f64() && x.fract() == 0.0 && x.abs() < 1e15 => Value::from(x as i64),
                Some(x) => Number::from_f64(x).map_or(Value::Null, Value::Number),
                None => v.clone(),
            },
        };
    }
    match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, c) in m {
                if !r.chance(rate * 2) {
                    out.insert(k.clone(), alter(r, c, rate));
                }
            }
            if r.chance(rate * 2) {
                out.insert(key(r), scalar(r));
            }
            Value::Object(out)
        }
        Value::Array(a) => {
            let mut out: Vec<Value> = a.iter().map(|c| alter(r, c, rate)).collect();
            if r.chance(rate * 2) {
                out.push(scalar(r));
            }
            Value::Array(out)
        }
        _ => v.clone(),
    }
}

// MARK: Edit states

pub struct Defaults {
    pub defaults: Value,
    pub nullable: Vec<String>,
}

pub fn engine_defaults() -> Defaults {
    let v: Value = serde_json::from_str(include_str!("engine_defaults.json")).unwrap();
    let nullable = v["nullable_paths"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect();
    Defaults { defaults: v["defaults"].clone(), nullable }
}

/// A finite double, mostly in `low..high`.
pub fn finite(r: &mut Random, low: f64, high: f64) -> f64 {
    if r.chance(10) { double(r) } else { r.between(low, high) }
}

fn color(r: &mut Random) -> border::BorderColor {
    border::BorderColor([r.below(256) as u8, r.below(256) as u8, r.below(256) as u8])
}

pub fn border_settings(r: &mut Random) -> border::BorderSettings {
    use spektrafilm_io::border::BorderStyle;
    let styles = [BorderStyle::Solid, BorderStyle::Film, BorderStyle::Keyline, BorderStyle::Film35];
    if r.chance(4) {
        return border::BorderSettings::default();
    }
    border::BorderSettings {
        enabled: r.bool(),
        style: styles[r.index(4)],
        export: r.bool(),
        instagram: r.bool(),
        width: finite(r, 0.0, 0.2),
        film_width: finite(r, 0.0, 0.2),
        line: finite(r, 0.0, 0.02),
        color: color(r),
        line_color: color(r),
    }
}

pub fn crop_settings(r: &mut Random) -> crop::CropSettings {
    use crop::{CropAspect, CropSettings, Rect, Size};
    let aspects = [
        CropAspect::Original, CropAspect::Freeform, CropAspect::Square, CropAspect::SixteenNine, CropAspect::ThreeTwo, CropAspect::FourThree,
        CropAspect::FiveFour, CropAspect::SevenFive, CropAspect::Custom,
    ];
    if r.chance(4) {
        return CropSettings::default();
    }
    let x = if r.chance(3) { 0.0 } else { finite(r, 0.0, 0.5) };
    let y = if r.chance(3) { 0.0 } else { finite(r, 0.0, 0.5) };
    let w = if r.chance(3) { 1.0 } else { finite(r, 0.05, 1.0) };
    let h = if r.chance(3) { 1.0 } else { finite(r, 0.05, 1.0) };
    CropSettings {
        rect: Rect::new(x, y, w, h),
        aspect: aspects[r.index(aspects.len())],
        custom: Size::new(finite(r, 1.0, 20.0), finite(r, 1.0, 20.0)),
        angle: if r.chance(2) { 0.0 } else { finite(r, -45.0, 45.0) },
    }
}

/// A params tree: the engine's defaults altered, or a random one with the
/// paths the rules read set now and then.
pub fn params(r: &mut Random, d: &Defaults) -> Value {
    let mut p = match r.below(4) {
        0 => value(r, 4),
        1 => d.defaults.clone(),
        _ => alter(r, &d.defaults, 40),
    };
    if !p.is_object() && r.chance(2) {
        p = Value::Object(Map::new());
    }
    for path in [
        "io.rotate_cw", "io.flip", "io.photo_long_side_px", "enlarger.m_filter_shift", "enlarger.y_filter_shift", "io.lens_correction.enabled",
        "prep.exposure_ev",
    ] {
        if r.chance(3) {
            let v = match r.below(6) {
                0 => Value::from(r.below(4)),
                1 => Value::Bool(r.bool()),
                2 => Value::Null,
                3 => Value::from(r.below(8000)),
                _ => number(r),
            };
            negs_state_oracle::json::set(&mut p, path, v);
        }
    }
    p
}

const FILMS: &[&str] = &["kodak_gold_200", "kodak_portra_400", "kodak_portra_endura", "fujifilm_crystal_archive_typeii", "", "é"];

pub fn film(r: &mut Random) -> String {
    if r.chance(5) { string(r) } else { FILMS[r.index(FILMS.len())].to_string() }
}

pub fn roll(r: &mut Random, s: &state::EditState) -> state::Roll {
    let shift = |r: &mut Random, path: &str| -> f64 {
        if r.chance(2) { s.value(path).as_f64().unwrap_or(0.0) } else { finite(r, -50.0, 50.0) }
    };
    state::Roll {
        id: string(r),
        magenta: shift(r, "enlarger.m_filter_shift"),
        yellow: shift(r, "enlarger.y_filter_shift"),
        paper: match r.below(3) {
            0 => None,
            1 => Some(s.paper.clone()),
            _ => Some(film(r)),
        },
    }
}

/// A random edit state. Its numbers are finite and a match's inputs are
/// never `Some(null)`: serde writes both so that it can't read them back
/// the same (null for a NaN; `"inputs":null` reads as None), so no JSON
/// crossing can keep them (edit_state.rs checks those apart).
pub fn edit_state(r: &mut Random, d: &Defaults) -> state::EditState {
    let mut s = state::EditState {
        params: params(r, d),
        film: film(r),
        paper: film(r),
        limit_preview: r.bool(),
        border: border_settings(r),
        crop: crop_settings(r),
        brightness_match: None,
        roll: None,
    };
    if r.chance(3) {
        s.brightness_match = Some(state::BrightnessMatch {
            ev: r.bool().then(|| finite(r, -3.0, 3.0)),
            inputs: if r.bool() { None } else { Some(if r.bool() { state::match_inputs(&s) } else { value(r, 3) }).filter(|v| !v.is_null()) },
            highlight_limited: r.bool(),
        });
    }
    if r.chance(3) {
        s.roll = Some(roll(r, &s));
    }
    s
}
