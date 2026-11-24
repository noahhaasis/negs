//! windows/src/match_cache.rs on the core (core/src/caches/match_cache,
//! through negs-core-rules) against the frozen Rust (core/diff/oracle/
//! match_cache.rs). windows/src/match_cache.rs is compiled here with its
//! own unit tests, on the oracle's EditState and EnginePaths.
//! - keys byte for byte: random requests (films, papers, the engine's real
//!   parameters and random values) on scratch files with awkward names, on
//!   files of this repository, and on missing ones; engine stamps;
//! - LRU sequences: random stores (NaN and infinite solves too), lookups,
//!   flushes and relaunches with small limits, the oracle on the real clock
//!   (each step waits for it to move, so no two uses tie) and the core on
//!   the time read just after the oracle's step (`now` passed in), so both
//!   order every use alike, across swapped files too; every lookup and count compared, and the
//!   entries files each side writes read by the other.
//! Seeded (NEGS_RULES_SEEDS, default 10,000; NEGS_RULES_SEED); each test
//! checks the core leaked nothing.
#![allow(dead_code)]

mod rules_common;
mod state_common;

mod state {
    pub use negs_state_oracle::state::*;
}
mod engine {
    pub use negs_state_oracle::engine::*;
}
mod session {
    pub use negs_state_oracle::session::*;
}

#[path = "../../../windows/src/match_cache.rs"]
mod match_cache;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use negs_core_rules::match_cache as core;
use negs_state_oracle::engine::EnginePaths;
use negs_state_oracle::match_cache as oracle;
use rules_common::{Leaks, Random, Tally, b, cases, first_seed};
use serde_json::Value;
use state_common::{edit_state, engine_defaults, serial, string};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("negs-diff-match-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const NAMES: &[&str] = &["a.nef", "IMG_0412.CR3", "é.raf", "中文 photo.dng", "😀", "x y", ".hidden", "a.b.c", "no extension", "ü\u{301}.arw"];

/// Files of this repository, as they are on disk.
const REAL: &[&str] = &["core/diff/Cargo.toml", "core/include/negs_core.h", "windows/src/match_cache.rs", "core/diff/tests/state_common/engine_defaults.json"];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn keys_and_engine_stamps() {
    let _s = serial();
    let leaks = Leaks::start();
    let d = engine_defaults();
    let dir = scratch("keys");
    let files: Vec<PathBuf> = NAMES
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let p = dir.join(n);
            std::fs::write(&p, vec![b'x'; i * 37]).unwrap();
            p
        })
        .collect();
    let mut t = Tally::default();
    let n = cases();
    for seed in first_seed()..first_seed() + n {
        let r = &mut Random(seed);
        let mut request = edit_state(r, &d);
        if r.chance(4) {
            request.params = state_common::value(r, 4);
        }
        let file = match r.below(6) {
            0 => repo().join(REAL[r.index(REAL.len())]),
            1 => dir.join(string(r)),
            2 => dir.join("missing.nef"),
            _ => files[r.index(files.len())].clone(),
        };
        let engine = if r.bool() { string(r) } else { "0.18.0|C:\\Negs\\helper.exe|1234|1700000000.5|C:\\data".to_string() };
        t.check("key", seed, oracle::key(&file, &request, &engine), match_cache::key(&file, &request, &engine));
        if r.chance(5) {
            // Touch a file: a new size and time name new solves.
            let f = &files[r.index(files.len())];
            std::fs::write(f, vec![b'y'; r.index(100)]).unwrap();
        }
        if r.chance(10) {
            let helper = match r.below(3) {
                0 => files[r.index(files.len())].clone(),
                1 => dir.join("no helper"),
                _ => repo().join(REAL[r.index(REAL.len())]),
            };
            let paths = EnginePaths { helper: helper.clone(), exporter: helper, data: dir.join(string(r)), fast_exporter: None };
            t.check("engine_stamp", seed, oracle::engine_stamp(&paths), match_cache::engine_stamp(&paths));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    t.finish("match cache keys", n);
    leaks.check("match cache keys");
}

/// An entries file as a map of key to (ev, limited), `used` left out (the
/// times of the two sides differ by microseconds). None when it isn't one.
fn entries(file: &Path) -> Option<BTreeMap<String, (Value, Value)>> {
    let v: Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    Some(v.as_object()?.iter().map(|(k, e)| (k.clone(), (e["ev"].clone(), e["limited"].clone()))).collect())
}

/// Waits for the clock to move, so each step's uses are later than the
/// last step's on both sides.
fn tick() {
    let start = std::time::SystemTime::now();
    while std::time::SystemTime::now() <= start {
        std::hint::spin_loop();
    }
}

#[test]
fn lru_sequences() {
    let _s = serial();
    let leaks = Leaks::start();
    let dir = scratch("lru");
    let mut t = Tally::default();
    // Each sequence is up to 120 steps on the clock: a twentieth of the cases.
    let n = (cases() / 20).max(100);
    let mut now;
    for seed in first_seed()..first_seed() + n {
        let r = &mut Random(seed);
        let limit = if r.bool() { 1 + r.index(12) } else { [9, 10, 19, 20, 29, 30][r.index(6)] };
        let (of, cf) = (dir.join(format!("{seed}/o/BrightnessMatches.json")), dir.join(format!("{seed}/c/BrightnessMatches.json")));
        let mut o = Arc::new(oracle::MatchCache::new(of.clone(), limit));
        let mut c = core::MatchCache::new(cf.clone(), limit);
        let keys: Vec<String> = (0..1 + r.index(if limit > 12 { 45 } else { 20 })).map(|i| if r.chance(6) { string(r) } else { format!("k{i}") }).collect();
        for _ in 0..r.index(if limit > 12 { 120 } else { 40 }) {
            tick();
            now = core::now();
            let key = keys[r.index(keys.len())].clone();
            match r.below(10) {
                0..=3 => {
                    let ev = if r.chance(20) { r.edge() } else { r.between(-3.0, 3.0) };
                    let limited = r.bool();
                    o.store(key.clone(), ev, limited);
                    c.store_at(&key, ev, limited, now);
                }
                4..=6 => {
                    let (os, cs) = (o.solved(&key), c.solved_at(&key, now));
                    t.check("solved", seed, os.map(|(e, l)| (b(e), l)), cs.map(|(e, l)| (b(e), l)));
                }
                7 | 8 => {
                    o.flush();
                    c.flush();
                    t.check("files", seed, entries(&of), entries(&cf));
                    t.check("no staging", seed, of.with_extension("tmp").exists(), cf.with_extension("tmp").exists());
                }
                _ => {
                    // Relaunch, each reading the file the other wrote.
                    o.flush();
                    c.flush();
                    let (om, cm) = (entries(&of), entries(&cf));
                    t.check("files", seed, om, cm);
                    if r.bool() && of.exists() && cf.exists() {
                        std::fs::copy(&of, dir.join("swap")).unwrap();
                        std::fs::copy(&cf, &of).unwrap();
                        std::fs::copy(dir.join("swap"), &cf).unwrap();
                    }
                    o = Arc::new(oracle::MatchCache::new(of.clone(), limit));
                    c = core::MatchCache::new(cf.clone(), limit);
                }
            }
        }
        // The oracle's count() is its tests' only: count what it finds.
        let mut found = std::collections::BTreeSet::new();
        for key in &keys {
            tick();
            now = core::now();
            let os = o.solved(key);
            if os.is_some() {
                found.insert(key.clone());
            }
            t.check("final solved", seed, os.map(|(e, l)| (b(e), l)), c.solved_at(key, now).map(|(e, l)| (b(e), l)));
        }
        t.check("final count", seed, found.len(), c.count());
    }
    // Damaged files read as empty on both sides.
    for (i, text) in [&b"{"[..], b"[]", b"{\"k\":{\"ev\":null,\"limited\":true,\"used\":1.0}}", b"{\"k\":{\"ev\":1,\"limited\":true,\"used\":1}}", b""].iter().enumerate() {
        let f = dir.join(format!("damaged{i}.json"));
        std::fs::write(&f, text).unwrap();
        let (os, cs) = (oracle::MatchCache::new(f.clone(), 5).solved("k"), core::MatchCache::new(f.clone(), 5).solved_at("k", 2.0));
        t.check("damaged", i as u64, os.map(|(e, l)| (b(e), l)), cs.map(|(e, l)| (b(e), l)));
        t.check("damaged count", i as u64, os.is_some() as usize, core::MatchCache::new(f, 5).count());
    }
    let _ = std::fs::remove_dir_all(&dir);
    t.finish("match cache LRU", n);
    leaks.check("match cache LRU");
}
