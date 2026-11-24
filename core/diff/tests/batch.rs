//! windows/src/batch.rs on the core (core/src/caches/batch, through
//! negs-core-rules) against the frozen Rust (core/diff/oracle/batch.rs).
//! windows/src/batch.rs itself is compiled here, its own unit tests
//! included, on the oracle's EditState and model types (the stand-in
//! modules below), so both sides share their inputs:
//! - file names for random choices and names (tokens, characters Windows
//!   refuses, controls, dots and spaces), and Options::save;
//! - batch_name on random paths and titles, numbered on random taken sets,
//!   unique on random name lists (case, dots, no extension);
//! - matched on random states and solves (the wrapper's and the core's
//!   whole-state call);
//! - Run's counts and words on random batches, exporting, lightroom_title,
//!   the dialog's message;
//! - journals written by each side and read by the other, byte for byte,
//!   and random and mutated journal texts read alike.
//! Seeded (NEGS_RULES_SEEDS, default 10,000; NEGS_RULES_SEED); each test
//! checks the core leaked nothing.
#![allow(dead_code)]

mod rules_common;
mod state_common;

mod state {
    pub use negs_state_oracle::state::*;
}
mod json {
    pub use negs_state_oracle::json::*;
}
mod engine {
    pub use negs_state_oracle::engine::*;
}
mod model {
    pub use negs_state_oracle::model::*;
}
mod session {
    pub use negs_state_oracle::session::*;
}

#[path = "../../../windows/src/batch.rs"]
mod batch;

use std::path::{Path, PathBuf};

use negs_state_oracle::batch as oracle;
use rules_common::{Leaks, Random, Tally, b, cases, first_seed};
use serde_json::Value;
use state_common::{Defaults, edit_state, engine_defaults, mutate, serial, string};

const NAME_PIECES: &[&str] = &[
    "{name}", "{film}", "IMG_0412", " ", ".", "..", "/", "\\", ":", "*", "?", "\"", "<", ">", "|", "\u{0}", "\u{1f}", "\u{7f}", "\u{85}", "é", "中",
    "😀", "\u{2028}", "\u{3000}", "a", "_", "-", "{nam", "e}", "{", "}",
];

fn piece_text(r: &mut Random) -> String {
    if r.chance(6) {
        return string(r);
    }
    (0..r.index(7)).map(|_| NAME_PIECES[r.index(NAME_PIECES.len())]).collect()
}

const FORMATS: &[&str] = &["png", "jpeg", "tiff", "jpg", "", "PNG", "tif"];
const ENGINES: &[&str] = &["reference", "fast", "", "Reference"];
const EDGES: &[&str] = &["full", "long", "short", "", "Long"];

fn options(r: &mut Random) -> oracle::Options {
    let mut o = oracle::Options::default();
    o.format = FORMATS[r.index(FORMATS.len())].into();
    o.engine = ENGINES[r.index(ENGINES.len())].into();
    o.edge = EDGES[r.index(EDGES.len())].into();
    o.pixels = if r.chance(5) { r.next() as u32 } else { r.below(70_000) as u32 };
    o.bit_depth = [8, 16, 0, 32, u32::MAX][r.index(5)];
    o.color_space = if r.chance(8) { r.next() as usize } else { r.index(6) };
    o.jpeg_quality = r.below(120) as u32;
    o.naming = oracle::Naming::ALL[r.index(3)];
    o.custom = piece_text(r);
    o.existing = oracle::Existing::ALL[r.index(3)];
    o.low_memory = r.bool();
    o.lightroom = match r.below(4) {
        0 => None,
        1 => Some("classic".into()),
        2 => Some("cc".into()),
        _ => Some(string(r)),
    };
    o
}

/// The oracle's value as the wrapper's type, through serde.
fn cross<A: serde::Serialize, B: serde::de::DeserializeOwned>(a: &A) -> B {
    serde_json::from_slice(&serde_json::to_vec(a).unwrap()).unwrap()
}

#[test]
fn names() {
    let _s = serial();
    let leaks = Leaks::start();
    let mut t = Tally::default();
    let n = cases();
    for seed in first_seed()..first_seed() + n {
        let r = &mut Random(seed);
        let o = options(r);
        let w: batch::Options = cross(&o);
        let (name, film) = (piece_text(r), piece_text(r));
        t.check("file_name", seed, o.file_name(&name, &film), w.file_name(&name, &film));
        let (os, ws) = (o.save(), w.save());
        t.check("save", seed, (os.bit_depth, os.color_space, os.jpeg_quality), (ws.bit_depth, ws.color_space, ws.jpeg_quality));
        t.check("naming keys", seed, (o.naming.key(), o.naming.title()), (w.naming.key(), w.naming.title()));
        t.check("existing keys", seed, (o.existing.key(), o.existing.title()), (w.existing.key(), w.existing.title()));
        let k = if r.bool() { piece_text(r) } else { ["nameFilm", "name", "custom", "number", "replace", "skip"][r.index(6)].to_string() };
        t.check("from_key", seed, (oracle::Naming::from_key(&k).key(), oracle::Existing::from_key(&k).key()), (batch::Naming::from_key(&k).key(), batch::Existing::from_key(&k).key()));
        t.check("options serde", seed, serde_json::to_string(&o).unwrap(), serde_json::to_string(&w).unwrap());

        // batch_name: a photo path, and a title that is mostly its file
        // name with a duplicate's number.
        let photo = path(r);
        let file = Path::new(&photo).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let title = match r.below(4) {
            0 => piece_text(r),
            1 => file.clone(),
            _ => format!("{file} ({})", r.below(20)),
        };
        t.check("batch_name", seed, oracle::batch_name(Path::new(&photo), &title), batch::batch_name(Path::new(&photo), &title));

        // numbered and unique.
        let names: Vec<String> = (0..r.index(8)).map(|_| file_name_like(r)).collect();
        let first = if names.is_empty() || r.chance(4) { file_name_like(r) } else { names[r.index(names.len())].clone() };
        let taken = |n: &str| names.iter().any(|x| x == n) || n.len() % 7 == 3;
        t.check("numbered", seed, oracle::numbered(&first, taken), batch::numbered(&first, taken));
        t.check("unique", seed, oracle::unique(&names), batch::unique(&names));
    }
    t.finish("batch names", n);
    leaks.check("batch names");
}

const PATH_PIECES: &[&str] = &[
    "/", "p", "IMG", ".CR3", ".", "..", "a.b.c", ".hidden", "x.", " ", "é", "中", "IMG_0412.NEF", "//", "photo", ".tar.gz", "\\", "C:",
];

fn path(r: &mut Random) -> String {
    if r.chance(10) {
        return string(r);
    }
    (0..r.index(6)).map(|_| PATH_PIECES[r.index(PATH_PIECES.len())]).collect()
}

fn file_name_like(r: &mut Random) -> String {
    match r.below(6) {
        0 => piece_text(r),
        1 => format!("{} 2.jpg", ["a", "A", "IMG"][r.index(3)]),
        _ => format!("{}{}", ["a", "A", "b", "IMG_1", "img_1", ".x", "", "a b"][r.index(8)], ["", ".jpg", ".JPG", ".tif", ".", ".tar.gz"][r.index(6)]),
    }
}

#[test]
fn matched() {
    let _s = serial();
    let leaks = Leaks::start();
    let d: Defaults = engine_defaults();
    let mut t = Tally::default();
    let n = cases();
    for seed in first_seed()..first_seed() + n {
        let r = &mut Random(seed);
        let mut s = edit_state(r, &d);
        if r.chance(3)
            && let Some(m) = s.brightness_match.as_mut()
        {
            m.ev = Some(if r.chance(4) { r.edge() } else { r.between(-3.0, 3.0) });
        }
        if r.chance(4) {
            let exposure = match r.below(5) {
                0 => Value::Null,
                1 => Value::from(r.edge()),
                2 => Value::String("1".into()),
                3 => Value::from(r.below(10) as i64 - 5),
                _ => Value::from(r.between(-4.0, 4.0)),
            };
            json::set(&mut s.params, state::PREP_EXPOSURE, exposure);
        }
        let ev = match r.below(6) {
            0 => r.edge(),
            1 => s.brightness_match.as_ref().and_then(|m| m.ev).unwrap_or(0.0) + r.between(-0.06, 0.06),
            _ => r.between(-4.0, 4.0),
        };
        let limited = r.bool();
        let inputs = if r.bool() { state::match_inputs(&s) } else { state_common::value(r, 3) };
        let o = oracle::matched(&s, ev, limited, inputs.clone()).map(|x| serde_json::to_string(&x).unwrap());
        let w = batch::matched(&s, ev, limited, inputs.clone()).map(|x| serde_json::to_string(&x).unwrap());
        t.check("matched", seed, o.clone(), w);
        // The core's whole-state call, on states their text carries (serde
        // writes a match's NaN or infinite ev as null, which reads as None).
        let text = serde_json::to_vec(&s).unwrap();
        if s.brightness_match.as_ref().is_some_and(|m| m.ev.is_some_and(|e| !e.is_finite())) {
            continue;
        }
        if let Some(core) = negs_core_rules::batch::matched_json(&text, ev, limited, &inputs) {
            t.check("matched_json", seed, o, Some(String::from_utf8(core).unwrap()));
        } else {
            t.check("matched_json refused only without a match", seed, s.brightness_match.is_none(), true);
        }
    }
    t.finish("batch matched", n);
    leaks.check("batch matched");
}

fn statuses(r: &mut Random) -> Vec<oracle::Status> {
    let all = [oracle::Status::Waiting, oracle::Status::Done, oracle::Status::Failed, oracle::Status::Skipped];
    let most = if r.chance(10) { 3000 } else { 15 };
    let n = if r.chance(8) { 0 } else { 1 + r.index(most) };
    let bias = r.index(4);
    (0..n).map(|_| if r.bool() { all[bias] } else { all[r.index(4)] }).collect()
}

fn o_item(r: &mut Random, d: &Defaults, status: oracle::Status, full: bool) -> oracle::Item {
    let photo = PathBuf::from(if full { path(r) } else { "/p/IMG.CR3".into() });
    oracle::Item {
        photo,
        variant: r.chance(3).then(|| string(r)),
        title: if full { string(r) } else { "IMG.CR3".into() },
        state: if full { edit_state(r, d) } else { Default::default() },
        needs_match: r.bool(),
        stores_match: r.bool(),
        rating: if r.bool() { None } else { Some([1, 5, -1, 0, i32::MIN, i32::MAX][r.index(6)]) },
        file_name: if full { file_name_like(r) } else { "IMG.jpg".into() },
        size: r.chance(2).then(|| (state_common::double(r), state_common::double(r))),
        look: r.chance(3).then(|| string(r)),
        status,
        reason: r.chance(4).then(|| string(r)),
        written: r.chance(4).then(|| PathBuf::from(path(r))),
    }
}

#[test]
fn runs_and_words() {
    let _s = serial();
    let leaks = Leaks::start();
    let d = engine_defaults();
    let mut t = Tally::default();
    let n = cases();
    for seed in first_seed()..first_seed() + n {
        let r = &mut Random(seed);
        let items = statuses(r).into_iter().map(|s| o_item(r, &d, s, false)).collect();
        let journal = oracle::Journal::new(PathBuf::from(path(r)), options(r), items);
        let (running, cancelled) = (r.bool(), r.chance(3));
        let o = oracle::Run { journal: journal.clone(), current: None, running, cancelled };
        let w = batch::Run { journal: cross(&journal), current: None, running, cancelled };
        t.check("counts", seed, (o.total(), o.finished(), o.exported(), o.failures().len(), o.skipped()), (w.total(), w.finished(), w.exported(), w.failures().len(), w.skipped()));
        let (orp, wrp) = (o.rule_parts(), w.rule_parts());
        t.check("fraction and rule", seed, (b(o.fraction()), b(orp.0), b(orp.1)), (b(w.fraction()), b(wrp.0), b(wrp.1)));
        t.check("needs_attention", seed, o.needs_attention(), w.needs_attention());
        t.check("title", seed, o.title(), w.title());
        t.check("progress_words", seed, o.progress_words(), w.progress_words());
        t.check("outcome_words", seed, o.outcome_words(), w.outcome_words());
        t.check("finished_words", seed, o.finished_words(), w.finished_words());

        let (finished, total) = (r.below(30) as usize, r.below(30) as usize);
        let lightroom = r.bool().then(|| piece_text(r));
        t.check("exporting", seed, oracle::exporting(finished, total, lightroom.as_deref()), batch::exporting(finished, total, lightroom.as_deref()));
        let k = if r.bool() { "classic".to_string() } else { piece_text(r) };
        t.check("lightroom_title", seed, oracle::lightroom_title(&k), batch::lightroom_title(&k));

        let opts = &journal.options;
        let (engine, size, save) = (opts.export_engine(), opts.size(), model::SaveSettings { bit_depth: [8, 16][r.index(2)], ..opts.save() });
        let space = ["sRGB", "Display P3", "Rec. 2020 (sRGB transfer)", "", "Adobe RGB (1998)"][r.index(5)];
        let count = if r.chance(4) { 1 } else { r.below(5000) as usize };
        t.check("message", seed, oracle::message(count, engine, size, save, space), batch::message(count, engine, size, save, space));
    }
    t.finish("batch runs", n);
    leaks.check("batch runs");
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("negs-diff-batch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn journals_cross_both_ways() {
    let _s = serial();
    let leaks = Leaks::start();
    let d = engine_defaults();
    let dir = scratch("journal");
    let mut t = Tally::default();
    // Files are slower than words: a tenth of the cases, at least 200.
    let n = (cases() / 10).max(200);
    for seed in first_seed()..first_seed() + n {
        let r = &mut Random(seed);
        let count = r.index(5);
        let items = (0..count).map(|_| o_item(r, &d, oracle::Status::Waiting, true)).collect();
        let mut journal = oracle::Journal::new(PathBuf::from(path(r)), options(r), items);
        if r.chance(8) {
            journal.schema = [0, 2, u32::MAX][r.index(3)];
        }
        let w: batch::Journal = cross(&journal);
        let (a, c) = (dir.join(format!("o{seed}/active-export.json")), dir.join(format!("w{seed}/x/active-export.json")));
        // Oracle writes, wrapper reads; wrapper writes, oracle reads.
        t.check("write", seed, journal.write(&a).is_ok(), w.write(&c).is_ok());
        t.check("same bytes", seed, std::fs::read(&a).ok(), std::fs::read(&c).ok());
        t.check("no staging left", seed, a.with_extension("json.tmp").exists(), c.with_extension("json.tmp").exists());
        let o_reads = oracle::Journal::read(&c).map(|j| serde_json::to_string(&j).unwrap());
        let w_reads = batch::Journal::read(&a).map(|j| serde_json::to_string(&j).unwrap());
        t.check("read across", seed, o_reads, w_reads);
        t.check("remaining", seed, journal.remaining(), w.remaining());
        oracle::Journal::remove(&c);
        batch::Journal::remove(&a);
        t.check("removed", seed, (a.exists(), c.exists()), (false, false));
    }
    // A NaN size: serde writes null, which neither side reads back.
    let mut it = o_item(&mut Random(7), &d, oracle::Status::Done, false);
    it.size = Some((f64::NAN, 1.0));
    let journal = oracle::Journal::new(dir.clone(), oracle::Options::default(), vec![it]);
    let mut finite = journal.clone();
    finite.items[0].size = Some((2.0, 1.0));
    let mut w: batch::Journal = cross(&finite);
    w.items[0].size = Some((f64::NAN, 1.0));
    let (a, c) = (dir.join("nan-o.json"), dir.join("nan-w.json"));
    journal.write(&a).unwrap();
    w.write(&c).unwrap();
    t.check("NaN journal bytes", 0, std::fs::read(&a).unwrap(), std::fs::read(&c).unwrap());
    t.check("NaN journal read", 0, oracle::Journal::read(&a).is_none(), batch::Journal::read(&c).is_none());
    let _ = std::fs::remove_dir_all(&dir);
    t.finish("batch journals", n);
    leaks.check("batch journals");
}

#[test]
fn journal_texts_read_alike() {
    let _s = serial();
    let leaks = Leaks::start();
    let d = engine_defaults();
    let mut t = Tally::default();
    let n = cases();
    let mut accepted = 0u64;
    for seed in first_seed()..first_seed() + n {
        let r = &mut Random(seed);
        let items = (0..r.index(3))
            .map(|_| {
                let status = [oracle::Status::Waiting, oracle::Status::Failed][r.index(2)];
                o_item(r, &d, status, true)
            })
            .collect();
        let journal = oracle::Journal::new(PathBuf::from(path(r)), options(r), items);
        let mut v = serde_json::to_value(&journal).unwrap();
        // Serde's other forms: fields left out, nulls, an enum as a map.
        if r.chance(3) {
            let o = v["options"].as_object_mut().unwrap();
            let keys: Vec<String> = o.keys().cloned().collect();
            for k in keys {
                if r.chance(3) {
                    o.remove(&k);
                }
            }
        }
        if r.chance(5) {
            v["options"]["naming"] = serde_json::json!({"custom": null});
        }
        if r.chance(5)
            && let Some(item) = v["items"].get_mut(0)
        {
            let k = ["status", "reason", "written", "variant", "rating", "size", "look"][r.index(7)];
            if r.bool() {
                item.as_object_mut().unwrap().remove(k);
            } else {
                item[k] = Value::Null;
            }
        }
        let base = if r.chance(4) { serde_json::to_vec_pretty(&v).unwrap() } else { serde_json::to_vec(&v).unwrap() };
        let text = if r.chance(3) { base } else { mutate(r, &base) };
        let o = serde_json::from_slice::<oracle::Journal>(&text).ok().map(|j| serde_json::to_vec(&j).unwrap());
        accepted += o.is_some() as u64;
        t.check("journal text", seed, o, negs_core_rules::batch::journal_rewrite(&text));
    }
    eprintln!("batch journal texts: {accepted} of {n} read by serde");
    t.finish("batch journal texts", n);
    leaks.check("batch journal texts");
}

#[test]
fn options_default_is_serdes() {
    let _s = serial();
    assert_eq!(negs_core_rules::batch::options_default_json(), serde_json::to_vec(&oracle::Options::default()).unwrap());
}
