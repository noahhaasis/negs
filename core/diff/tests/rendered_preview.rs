//! windows/src/rendered_preview.rs on the core (core/src/caches/preview,
//! through negs-core-rules) against the frozen Rust (core/diff/oracle/
//! rendered_preview.rs). windows/src/rendered_preview.rs is compiled here
//! on the oracle's EditState and EnginePaths (its own unit tests run in
//! caches_units; this file runs without libtest, for the leak checks).
//! - checksums: DefaultHasher (SipHash-1-3) over random bytes of random
//!   lengths, 0, every length to 80 and past 64 KB;
//! - keys byte for byte: random states (NaNs too, which the wrapper keys by
//!   the rendered value) on a scratch engine folder of nested files with
//!   awkward names that random steps add to, change and remove, and on this
//!   repository's real data folder;
//! - frame files: random frames (sizes and values out of range too) saved
//!   by each side and loaded by the other, byte for byte, then damaged
//!   (header bytes, pixel bytes, truncated, extended) and loaded alike.
//! Seeded (NEGS_RULES_SEEDS, default 10,000; NEGS_RULES_SEED); each test
//! checks the core leaked nothing.
#![allow(dead_code, unused_imports)] // the windows files' test modules compile here without their tests

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

#[path = "../../../windows/src/rendered_preview.rs"]
mod rendered_preview;

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use negs_core_rules::rendered_preview as core;
use negs_state_oracle::engine::EnginePaths;
use negs_state_oracle::rendered_preview as oracle;
use rules_common::{Leaks, Random, Tally, cases, first_seed};
use state_common::{edit_state, engine_defaults, mutate, serial, string};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("negs-diff-preview-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// rendered_preview.rs's checksum, as it is there (private to it).
fn rust_checksum(bytes: &[u8]) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    hash.finish()
}

fn checksums() {
    let _s = serial();
    let leaks = Leaks::start();
    let mut t = Tally::default();
    let n = cases();
    let mut bytes = Vec::new();
    for seed in first_seed()..first_seed() + n {
        let r = &mut Random(seed);
        let len = match r.below(10) {
            0 => 0,
            1 => 65_536 + r.index(70_000),
            2 => r.index(4096),
            _ => r.index(81),
        };
        bytes.clear();
        bytes.extend((0..len).map(|_| r.next() as u8));
        let at = if r.bool() { 0 } else { r.index(8) };
        let slice = &bytes[at.min(len)..];
        t.check("checksum", seed, rust_checksum(slice), core::checksum(slice));
    }
    for len in 0..=80usize {
        let bytes: Vec<u8> = (0..len as u8).collect();
        t.check("checksum by length", len as u64, rust_checksum(&bytes), core::checksum(&bytes));
    }
    t.finish("preview checksums", n);
    leaks.check("preview checksums");
}

const NAMES: &[&str] = &["film.json", "paper.json", "é.npz", "中.lut", "😀", "a b", ".hidden", "x.y.z", "README", "ü\u{301}"];

/// A scratch engine: a source, a helper and a data folder like the real
/// one (profiles in nested folders).
struct Engine {
    dir: PathBuf,
    source: PathBuf,
    paths: EnginePaths,
    files: Vec<PathBuf>,
}

impl Engine {
    fn new(dir: &Path) -> Engine {
        let source = dir.join("photo.raw");
        let helper = dir.join("spektrafilm-native");
        let data = dir.join("data");
        std::fs::write(&source, b"original").unwrap();
        std::fs::write(&helper, b"helper").unwrap();
        let mut files = Vec::new();
        for (i, sub) in ["profiles", "profiles/kodak", "profiles/kodak/é", "luts", "presets/中 文"].iter().enumerate() {
            std::fs::create_dir_all(data.join(sub)).unwrap();
            for name in &NAMES[..3 + i] {
                let f = data.join(sub).join(name);
                std::fs::write(&f, name.as_bytes()).unwrap();
                files.push(f);
            }
        }
        Engine { dir: dir.to_path_buf(), source, paths: EnginePaths { helper: helper.clone(), exporter: helper, data, fast_exporter: None }, files }
    }

    fn step(&mut self, r: &mut Random) {
        match r.below(8) {
            0 => {
                let f = self.files[r.index(self.files.len())].clone();
                std::fs::write(&f, vec![b'x'; r.index(50)]).unwrap();
            }
            1 => {
                let f = self.paths.data.join(["profiles", "luts", "profiles/kodak/é"][r.index(3)]).join(string(r).replace(['/', '\0'], "_"));
                if std::fs::write(&f, b"new").is_ok() {
                    self.files.push(f);
                }
            }
            2 if self.files.len() > 4 => {
                let f = self.files.swap_remove(r.index(self.files.len()));
                let _ = std::fs::remove_file(f);
            }
            3 => std::fs::write(&self.source, vec![b's'; r.index(20)]).unwrap(),
            4 => std::fs::write(&self.paths.helper, vec![b'h'; r.index(20)]).unwrap(),
            _ => {}
        }
    }
}

fn keys() {
    let _s = serial();
    let leaks = Leaks::start();
    let d = engine_defaults();
    let dir = scratch("keys");
    let mut engine = Engine::new(&dir);
    let mut t = Tally::default();
    // Every key reads the folder twice: a fifth of the cases.
    let n = (cases() / 5).max(200);
    let (mut some, mut nan) = (0, 0);
    for seed in first_seed()..first_seed() + n {
        let r = &mut Random(seed);
        let mut s = edit_state(r, &d);
        if r.chance(10) {
            // A NaN the state's text can't carry: the wrapper keys it by
            // its rendered value.
            s.crop.angle = f64::NAN;
            nan += 1;
        }
        engine.step(r);
        let source = if r.chance(20) { dir.join("missing.raw") } else { engine.source.clone() };
        let o = oracle::key(&source, &s, &engine.paths);
        some += o.is_some() as u64;
        t.check("key", seed, o, rendered_preview::key(&source, &s, &engine.paths));
    }
    // The real data folder.
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let real = EnginePaths { helper: repo.join("core/include/negs_core.h"), exporter: repo.join("Cargo.toml"), data: repo.join("data"), fast_exporter: None };
    for seed in 0..20 {
        let s = edit_state(&mut Random(seed), &d);
        let o = oracle::key(&repo.join("README.md"), &s, &real);
        assert!(o.is_some(), "the real data folder keys");
        t.check("real key", seed, o, rendered_preview::key(&repo.join("README.md"), &s, &real));
    }
    eprintln!("preview keys: {some} of {n} keyed ({nan} with a NaN)");
    let _ = std::fs::remove_dir_all(&dir);
    t.finish("preview keys", n);
    leaks.check("preview keys");
}

fn dimension(r: &mut Random) -> u32 {
    match r.below(20) {
        0 => 0,
        1 => 8192,
        2 => 8193,
        3 => r.next() as u32,
        _ => 1 + r.below(24) as u32,
    }
}

fn size(r: &mut Random) -> f64 {
    match r.below(12) {
        0 => r.edge(),
        1 => 1.0,
        2 => 0.999,
        _ => r.between(1.0, 9000.0).round(),
    }
}

fn o_frame(r: &mut Random) -> oracle::Frame {
    let (width, height) = (dimension(r), dimension(r));
    let mut count = (width as usize).min(8192) * (height as usize).min(8192) * 4;
    if count > 4_000_000 || r.chance(15) {
        count = r.index(64);
    }
    oracle::Frame {
        width,
        height,
        photo_width: size(r),
        photo_height: size(r),
        source_width: size(r),
        source_height: size(r),
        working_width: size(r),
        working_height: size(r),
        raw: r.bool(),
        display_space: if r.bool() { negs_state_oracle::picture::WIDE_DISPLAY_SPACE.into() } else { string(r) },
        lens_correction_supported: r.bool(),
        pixels: (0..count).map(|_| r.next() as u8).collect(),
    }
}

fn c_frame(f: &oracle::Frame) -> core::Frame {
    core::Frame {
        width: f.width,
        height: f.height,
        photo_width: f.photo_width,
        photo_height: f.photo_height,
        source_width: f.source_width,
        source_height: f.source_height,
        working_width: f.working_width,
        working_height: f.working_height,
        raw: f.raw,
        display_space: f.display_space.clone(),
        lens_correction_supported: f.lens_correction_supported,
        pixels: f.pixels.clone(),
    }
}

/// A frame as both sides give it: its header's JSON (bits of its numbers
/// included) and its pixels.
fn view_o(f: &oracle::Frame) -> (String, Vec<u8>) {
    (serde_json::to_string(f).unwrap(), f.pixels.clone())
}
fn view_c(f: &core::Frame) -> (String, Vec<u8>) {
    (serde_json::to_string(f).unwrap(), f.pixels.clone())
}

/// The oracle's load of `file` for this source, state and engine.
fn oracle_load(file: &Path, engine: &Engine, s: &state::EditState) -> Option<(String, Vec<u8>)> {
    let cache = oracle::RenderedPreviewCache::new(file.to_path_buf());
    let (tx, rx) = std::sync::mpsc::channel();
    cache.load(engine.source.clone(), s.clone(), engine.paths.clone(), move |frame| {
        let _ = tx.send(frame.as_ref().map(view_o));
    });
    rx.recv().unwrap()
}

fn frames_cross_both_ways() {
    let _s = serial();
    let leaks = Leaks::start();
    let d = engine_defaults();
    let dir = scratch("frames");
    let engine = Engine::new(&dir);
    let mut t = Tally::default();
    let n = (cases() / 5).max(200);
    let mut loaded = 0;
    for seed in first_seed()..first_seed() + n {
        let r = &mut Random(seed);
        let s = edit_state(r, &d);
        let key = oracle::key(&engine.source, &s, &engine.paths).unwrap();
        let frame = o_frame(r);
        let (of, cf) = (dir.join(format!("o/{seed}.cache")), dir.join(format!("c/{seed}/frame.cache")));
        // Oracle saves, core loads; core saves, oracle loads.
        let written = core::write(&cf, &key, &c_frame(&frame));
        let cache = oracle::RenderedPreviewCache::new(of.clone());
        cache.save(key.clone(), frame);
        cache.flush();
        t.check("saved", seed, of.exists(), written.is_ok() && cf.exists());
        t.check("same bytes", seed, std::fs::read(&of).ok(), std::fs::read(&cf).ok());
        t.check("no staging", seed, of.with_extension("tmp").exists(), cf.with_extension("tmp").exists());
        let o = oracle_load(&cf, &engine, &s);
        loaded += o.is_some() as u64;
        t.check("load across", seed, o, core::read(&of, &key).ok().as_ref().map(view_c));
        let other = edit_state(r, &d);
        let other_key = oracle::key(&engine.source, &other, &engine.paths).unwrap();
        t.check("other key", seed, oracle_load(&cf, &engine, &other).is_some(), core::read(&of, &other_key).is_ok());

        // Damaged.
        if let Ok(bytes) = std::fs::read(&of) {
            let header = bytes.iter().position(|&b| b == b'\n').unwrap_or(bytes.len());
            let damaged = match r.below(5) {
                0 => mutate(r, &bytes[..header]).into_iter().chain(bytes[header..].iter().copied()).collect(),
                1 if bytes.len() > header + 1 => {
                    let mut b = bytes.clone();
                    let at = header + 1 + r.index(bytes.len() - header - 1);
                    b[at] ^= 1 << r.below(8);
                    b
                }
                2 => bytes[..r.index(bytes.len() + 1)].to_vec(),
                3 => [&bytes[..], &[r.next() as u8][..]].concat(),
                _ => bytes.iter().map(|&b| if b == b'\n' { b' ' } else { b }).collect(),
            };
            std::fs::write(&of, &damaged).unwrap();
            std::fs::write(&cf, &damaged).unwrap();
            t.check("damaged", seed, oracle_load(&cf, &engine, &s), core::read(&of, &key).ok().as_ref().map(view_c));
        }
        let _ = std::fs::remove_file(&of);
        let _ = std::fs::remove_file(&cf);
    }
    eprintln!("preview frames: {loaded} of {n} loaded back");
    let _ = std::fs::remove_dir_all(&dir);
    t.finish("preview frames", n);
    leaks.check("preview frames");
}

/// Without libtest (`harness = false`, see caches_units.rs): each test in
/// turn; a filter argument runs those whose names contain it.
fn main() {
    let filter = std::env::args().skip(1).find(|a| !a.starts_with('-'));
    let tests: &[(&str, fn())] = &[
        ("checksums", checksums),
        ("keys", keys),
        ("frames_cross_both_ways", frames_cross_both_ways),
    ];
    for (name, test) in tests {
        if filter.as_deref().is_some_and(|f| !name.contains(f)) {
            continue;
        }
        eprintln!("test {name} ...");
        test();
        eprintln!("test {name} ... ok");
    }
}
