//! model/compare.rs's pure helpers, the Odin core (negs-core-rules'
//! model_compare) against the frozen Rust: seeded images halved, and camera
//! JPEGs laid over RAW frames, every byte and size compared.
//! NEGS_RULES_SEEDS cases (10,000).
//!
//! The rest of windows/src/model/compare.rs is the model's frame pipeline,
//! which can't build here, so oracle/model_compare_pure.rs holds verbatim
//! copies of the two functions; the first test checks they still read as
//! the frozen source (oracle/model_compare.rs.txt) does.

mod rules_common;

use negs_core_diff::model_compare as oracle;
use negs_core_rules::model_compare as core;
use rules_common::{Leaks, Random, Tally, cases, first_seed};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn the_copies_are_the_frozen_source() {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle");
    let source = std::fs::read_to_string(here.join("model_compare.rs.txt")).unwrap();
    let copies = std::fs::read_to_string(here.join("model_compare_pure.rs")).unwrap();
    for name in ["fn halved<", "pub fn over_frame("] {
        let start = copies.find(name).unwrap_or_else(|| panic!("{name} is copied"));
        let head = copies[..start].rfind("\n\n").map_or(0, |i| i + 2);
        let end = start + copies[start..].find("\n}\n").expect("the copy ends") + 3;
        assert!(source.contains(&copies[head..end]), "{name}: the copy differs from oracle/model_compare.rs.txt");
    }
}

fn image(r: &mut Random, width: u32, height: u32) -> Vec<u8> {
    let flat = r.chance(5);
    let v = r.next() as u8;
    (0..width as usize * height as usize * 4).map(|_| if flat { v } else { [0, 1, 2, 254, 255, r.next() as u8][r.index(6)] }).collect()
}

fn side(r: &mut Random, most: u64) -> u32 {
    match r.below(8) {
        0 => 1,
        1 => 2,
        2 => 3,
        _ => 1 + r.below(most) as u32,
    }
}

type Over = Option<(Vec<u8>, u32, u32)>;

/// The result, or that it panicked (the image crate's resize, on both sides).
fn run(f: impl FnOnce() -> Over) -> Result<Over, ()> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|_| ())
}

#[test]
fn model_compare_matches_the_oracle() {
    let leaks = Leaks::start();
    let (n, first) = (cases(), first_seed());
    let mut t = Tally::default();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    for seed in first..first + n {
        let mut r = Random(seed);
        let (w, h) = (side(&mut r, 48), side(&mut r, 48));
        let pixels = image(&mut r, w, h);
        let picture = image::RgbaImage::from_raw(w, h, pixels).unwrap();
        let (o, c) = (oracle::halved_oracle(&picture), core::halved(&picture));
        t.check("halved", seed, (o.dimensions(), o.into_raw()), (c.dimensions(), c.into_raw()));

        // Camera JPEGs over RAW frames: the same shape (within 1%) or not;
        // empty ones and short buffers too.
        let (w, h) = (if r.chance(30) { 0 } else { side(&mut r, 40) }, if r.chance(30) { 0 } else { side(&mut r, 40) });
        let mut rgba = image(&mut r, w, h);
        if r.chance(20) && !rgba.is_empty() {
            rgba.pop();
        }
        let frame = match r.below(5) {
            0 => ((w as f64 * 100.0 * r.between(0.995, 1.005)) as u32, h * 100),
            1 => (0, 0),
            2 => (r.next() as u32, r.next() as u32),
            _ => (side(&mut r, 9000), side(&mut r, 9000)),
        };
        let (o, c) = (run(|| oracle::over_frame(&rgba, w, h, frame)), run(|| core::over_frame(&rgba, w, h, frame)));
        t.check("over_frame", seed, o, c);
    }
    std::panic::set_hook(hook);
    t.finish("model_compare", n);
    leaks.check("model_compare");
}

/// A large develop, as the canvas halves it: odd sides repeat their last row and column.
#[test]
fn a_large_odd_image_halves_alike() {
    let mut r = Random(99);
    let (w, h) = (1001, 667);
    let picture = image::RgbaImage::from_raw(w, h, image(&mut r, w, h)).unwrap();
    assert_eq!(oracle::halved_oracle(&picture), core::halved(&picture));
}
