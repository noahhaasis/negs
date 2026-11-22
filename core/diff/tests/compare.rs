//! compare, the Odin core (negs-core-rules) against the frozen Rust
//! (oracle/compare.rs, windows/src at 00b0d0d): seeded folders with
//! duplicates, selections, offers, words, differences, steps, frame numbers,
//! Original's settings and the arrangements, every output compared, floats
//! bit for bit. NEGS_RULES_SEEDS cases (10,000).
//!
//! Paths are the library's (no `//`, `/./` or trailing `/`): the core
//! compares them byte for byte, Rust's `Path` component by component, and
//! the two agree on such paths.

mod rules_common;

use negs_core_diff::compare as oracle;
use negs_core_rules::compare as core;
use negs_library::Item;
use rules_common::{Leaks, Random, Tally, b, bs, cases, first_seed};

const PHOTOS: [&str; 9] = [
    "/Users/n/Pictures/Roll 1/DSC_0001.NEF",
    "/Users/n/Pictures/Roll 1/DSC_0002.NEF",
    "/Users/n/Pictures/Roll 1/DSC_0003.JPG",
    "/Users/n/Pictures/Ålesund/IMG_0004.CR3",
    "/Users/n/Pictures/東京/📷 5.dng",
    "C:\\Users\\Noah\\Pictures\\a.NEF",
    "relative/b.tiff",
    "/Users/n/Pictures/Roll 1/DSC_0001.nef",
    "x",
];
const VARIANTS: [&str; 5] = ["7F3A", "0001", "", "dup-ü", "b"];

fn item(r: &mut Random) -> Item {
    let photo = PHOTOS[r.index(PHOTOS.len())];
    let variant = if r.chance(3) { Some(VARIANTS[r.index(VARIANTS.len())].to_string()) } else { None };
    Item { photo: photo.into(), variant }
}

/// A folder as the library lists it: photographs in order, each with its
/// duplicates after it; sometimes anything a host might pass.
fn folder(r: &mut Random) -> Vec<Item> {
    if r.chance(6) {
        return (0..r.index(12)).map(|_| item(r)).collect();
    }
    let mut out = vec![];
    let count = r.index(PHOTOS.len() + 1);
    let mut photos: Vec<&str> = PHOTOS.to_vec();
    for k in 0..count {
        let j = k + r.index(photos.len() - k);
        photos.swap(k, j);
        out.push(Item { photo: photos[k].into(), variant: None });
        let copies = if r.chance(20) { 30 } else if r.chance(3) { r.index(4) } else { 0 };
        for c in 0..copies {
            out.push(Item { photo: photos[k].into(), variant: Some(format!("{c:04X}")) });
        }
    }
    out
}

fn pick(r: &mut Random, items: &[Item]) -> Item {
    if items.is_empty() || r.chance(10) { item(r) } else { items[r.index(items.len())].clone() }
}

fn selection(r: &mut Random, items: &[Item]) -> Vec<Item> {
    let n = [0, 1, 1, 2, 2, 2, 3, 5][r.index(8)];
    (0..n).map(|_| pick(r, items)).collect()
}

fn number(i: &Item) -> String {
    format!("{}{}", i.photo.display(), i.variant.as_deref().unwrap_or("·"))
}

fn offer_o(o: oracle::Offer) -> (String, bool, Option<(Item, Item)>, bool) {
    (o.title, o.enabled, o.pair, o.with_original)
}

fn offer_c(o: core::Offer) -> (String, bool, Option<(Item, Item)>, bool) {
    (o.title, o.enabled, o.pair, o.with_original)
}

fn facts(r: &mut Random) -> Option<[Option<String>; 5]> {
    if r.chance(6) {
        return None;
    }
    let values = ["NIKON Z 6", "ISO 400", "1/250 s", "f/2.8", "35 mm", "", "ISO 400 "];
    Some(std::array::from_fn(|_| if r.chance(3) { None } else { Some(values[r.index(values.len())].to_string()) }))
}

fn rules(r: &mut Random, seed: u64, t: &mut Tally) {
    let items = folder(r);
    let (a, z) = (pick(r, &items), pick(r, &items));
    let (fa, fz) = (r.bool(), r.bool());
    let side_o = |i: &Item, original: bool| if original { oracle::Side::Original(i.clone()) } else { oracle::Side::Photo(i.clone()) };
    let side_c = |i: &Item, original: bool| if original { core::Side::Original(i.clone()) } else { core::Side::Photo(i.clone()) };
    let active_first = r.bool();
    let co = oracle::Comparison::new(side_o(&a, fa), side_o(&z, fz), active_first, oracle::Origin::Grid);
    let cc = core::Comparison::new(side_c(&a, fa), side_c(&z, fz), active_first, core::Origin::Grid);
    t.check("Comparison::against_original", seed, co.against_original(), cc.against_original());
    t.check("Comparison::edited", seed, co.edited().clone(), cc.edited().clone());
    t.check("Comparison::active", seed, (co.active().item().clone(), co.active().is_original()), (cc.active().item().clone(), cc.active().is_original()));

    let targets = selection(r, &items);
    let current = if r.chance(4) { None } else { Some(pick(r, &items)) };
    let clicked = if !targets.is_empty() && r.bool() { targets[0].clone() } else { pick(r, &items) };
    let (can, with) = (r.bool(), r.bool());
    let original = ["original", "camera JPEG", "", "東京"][r.index(4)];
    t.check(
        "offer",
        seed,
        offer_o(oracle::offer(&targets, current.as_ref(), &clicked, &number, can, with, original)),
        offer_c(core::offer(&targets, current.as_ref(), &clicked, &number, can, with, original)),
    );
    t.check("word", seed, oracle::word(&targets, &number, can), core::word(&targets, &number, can));

    let (pa, pz) = (facts(r), facts(r));
    let films = ["kodak-portra-400", "ilford-hp5", ""];
    let film = (films[r.index(3)], films[r.index(3)]);
    let papers = [None, Some("kodak-2383"), Some(""), Some("fuji-crystal-archive")];
    let paper = (papers[r.index(4)], papers[r.index(4)]);
    t.check("differences", seed, oracle::differences(pa.as_ref(), pz.as_ref(), film, paper), core::differences(pa.as_ref(), pz.as_ref(), film, paper));
    let comparing = r.bool();
    t.check("menu_title", seed, oracle::menu_title(comparing), core::menu_title(comparing));

    let order = if r.bool() { items.clone() } else { selection(r, &items) };
    let (from, first) = (pick(r, &items), pick(r, &items));
    let offset = [-1isize, 1, -2, 2, 0, isize::MIN / 2, isize::MAX / 2][r.index(7)];
    t.check("step", seed, oracle::step(&from, &first, &order, offset), core::step(&from, &first, &order, offset));
    let it = pick(r, &items);
    t.check("frame_number", seed, oracle::frame_number(&items, &it), core::frame_number(&items, &it));

    let keys = ["hold", "split", "sideBySide", "side-by-side", "", "raw", "cameraJpeg", "Hold", "camera-jpeg"];
    let key = keys[r.index(keys.len())];
    t.check("OriginalMode::from_key", seed, oracle::OriginalMode::from_key(key) as u8, core::OriginalMode::from_key(key) as u8);
    t.check("OriginalReference::from_key", seed, oracle::OriginalReference::from_key(key) as u8, core::OriginalReference::from_key(key) as u8);
    for (o, c) in oracle::OriginalMode::ALL.into_iter().zip(core::OriginalMode::ALL) {
        t.check("OriginalMode::key", seed, o.key(), c.key());
        t.check("OriginalMode::title", seed, o.title(), c.title());
        t.check("OriginalMode::help", seed, o.help(original), c.help(original));
    }
    for (o, c) in oracle::OriginalReference::ALL.into_iter().zip(core::OriginalReference::ALL) {
        t.check("OriginalReference::key", seed, o.key(), c.key());
        t.check("OriginalReference::title", seed, o.title(), c.title());
    }
}

fn aspect(r: &mut Random) -> f64 {
    match r.below(6) {
        0 => [1.5, 2.0 / 3.0, 1.0, 4.0 / 3.0, 0.75, 16.0 / 9.0][r.index(6)],
        1 => r.edge(),
        _ => r.between(0.1, 6.0),
    }
}

fn geometry(r: &mut Random, seed: u64, t: &mut Tally) {
    let canvas = (r.whole(0.0, 4000.0), r.whole(0.0, 3000.0));
    let aspects = (aspect(r), aspect(r));
    let room = (r.float(-10.0, 3000.0), r.float(-10.0, 3000.0));
    let (fo, fc) = (oracle::fit(aspects.0, room), core::fit(aspects.0, room));
    t.check("fit", seed, bs([fo.0, fo.1]), bs([fc.0, fc.1]));
    let frames = |f: [[f64; 4]; 2]| f.map(bs);
    t.check("row_layout", seed, frames(oracle::row_layout(canvas, aspects)), frames(core::row_layout(canvas, aspects)));
    t.check("column_layout", seed, frames(oracle::column_layout(canvas, aspects)), frames(core::column_layout(canvas, aspects)));
    let (lo, lc) = (oracle::layout(canvas, aspects), core::layout(canvas, aspects));
    t.check("layout", seed, (lo.0 as u8, frames(lo.1)), (lc.0 as u8, frames(lc.1)));
    t.check("slider_frame", seed, bs(oracle::slider_frame(canvas, aspects.0)), bs(core::slider_frame(canvas, aspects.0)));
}

#[test]
fn compare_matches_the_oracle() {
    let leaks = Leaks::start();
    let (n, first) = (cases(), first_seed());
    let mut t = Tally::default();
    for seed in first..first + n {
        let mut r = Random(seed);
        rules(&mut r, seed, &mut t);
        geometry(&mut r, seed, &mut t);
    }
    t.finish("compare", n);
    leaks.check("compare");
}

#[test]
fn the_constants_are_the_oracles() {
    assert_eq!(core::PARTS, oracle::PARTS);
    let ours = [core::CAPTION, core::LABEL, core::STACKED_CAPTION, core::GAP, core::STACKED_GAP, core::SIDE, core::BOTTOM];
    let theirs = [oracle::CAPTION, oracle::LABEL, oracle::STACKED_CAPTION, oracle::GAP, oracle::STACKED_GAP, oracle::SIDE, oracle::BOTTOM];
    assert_eq!(ours.map(b), theirs.map(b));
}
