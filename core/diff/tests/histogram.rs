//! The scopes, Odin core (negs_core_rules::histogram) against the Rust
//! oracle (windows/src/histogram.rs as it was, core/diff/oracle). Seeded
//! pictures, crops (straightened, out of range, NaN now and then),
//! orientations and sample budgets (NEGS_RULES_SEEDS, default 10,000;
//! NEGS_RULES_SEED): every bin of the histogram and the waveform, the
//! sample count, the peak and every byte of the waveform's raster.

mod picture_common;

use negs_core_diff_picture::{crop as ocrop, histogram as oracle};
use negs_core_rules::{crop as ccrop, histogram as core};
use picture_common::{Random, Tally, cases, gate, seed};

fn crop(r: &mut Random) -> ccrop::CropSettings {
    let rect = match r.below(6) {
        0 => ccrop::Rect::UNIT,
        1 => ccrop::Rect::new(r.fraction(), r.fraction(), r.fraction(), r.fraction()),
        _ => {
            let (x, y) = (r.range(0.0, 0.9), r.range(0.0, 0.9));
            ccrop::Rect::new(x, y, r.range(0.0, 1.0 - x), r.range(0.0, 1.0 - y))
        }
    };
    let angle = match r.below(8) {
        0..=3 => 0.0,
        4 => r.wild(),
        _ => r.angle(),
    };
    ccrop::CropSettings { rect, angle, ..ccrop::CropSettings::default() }
}

fn o_crop(c: &ccrop::CropSettings) -> ocrop::CropSettings {
    ocrop::CropSettings { rect: ocrop::Rect::new(c.rect.x, c.rect.y, c.rect.width, c.rect.height), angle: c.angle, ..ocrop::CropSettings::default() }
}

fn picture(r: &mut Random, w: usize, h: usize) -> Vec<u8> {
    match r.below(3) {
        0 => (0..w * h * 4).map(|_| r.next() as u8).collect(),
        1 => (0..w * h * 4).map(|i| ((i / 4 % w) * 255 / w.max(1)) as u8 ^ (i % 4) as u8).collect(),
        _ => {
            let c = [r.next() as u8, r.next() as u8, r.next() as u8, 255];
            (0..w * h * 4).map(|i| if r.chance(20) { r.next() as u8 } else { c[i % 4] }).collect()
        }
    }
}

fn compare(t: &mut Tally, what: &dyn Fn() -> String, o: &oracle::Scopes, c: &core::Scopes) {
    t.eq(what, &o.histogram.red[..], &c.histogram.red[..]);
    t.eq(what, &o.histogram.green[..], &c.histogram.green[..]);
    t.eq(what, &o.histogram.blue[..], &c.histogram.blue[..]);
    t.eq(what, (o.waveform.width, o.waveform.samples), (c.waveform.width, c.waveform.samples));
    t.eq(what, &o.waveform.red, &c.waveform.red);
    t.eq(what, &o.waveform.green, &c.waveform.green);
    t.eq(what, &o.waveform.blue, &c.waveform.blue);
    t.eq(what, o.histogram.peak(), c.histogram.peak());
    let (a, b) = (o.waveform.raster(), c.waveform.raster());
    t.compared += a.len() as u64;
    if a != b {
        t.eq(what, a.iter().position(|p| !b.contains(p)), None);
    }
}

fn case(r: &mut Random, t: &mut Tally, i: usize) {
    let (w, h) = match r.below(8) {
        0 => (r.below(3) as usize, r.below(3) as usize),
        1 => (1 + r.below(2000) as usize, 1 + r.below(8) as usize),
        _ => (1 + r.below(300) as usize, 1 + r.below(300) as usize),
    };
    let mut rgba = picture(r, w, h);
    if r.chance(40) {
        rgba.truncate(rgba.len().saturating_sub(1));
    }
    let c = crop(r);
    let oc = o_crop(&c);
    let (turns, flip) = (match r.below(8) {
        0 => r.int(-9, 9) as i32,
        _ => r.below(4) as i32,
    }, r.chance(3));
    let samples = match r.below(8) {
        0 => 0,
        1 => 1 + r.below(4) as usize,
        2 => 1 << 20,
        // Any budget, but the whole of a wild crop (a rect of 1e300) is
        // as slow in both as it is long: only for crops in the picture.
        3 if [c.rect.x, c.rect.y, c.rect.width, c.rect.height].iter().all(|v| (0.0..=1.0).contains(v)) => usize::MAX,
        _ => 1 + r.below(20_000) as usize,
    };
    let built = r.chance(3);
    let (oo, co) = if built {
        (ocrop::Orientation { turns, flip }, ccrop::Orientation { turns, flip })
    } else {
        (ocrop::Orientation::new(turns, flip), ccrop::Orientation::new(turns, flip))
    };
    let o = oracle::Scopes::shown(&rgba, w, h, &oc, oo, samples);
    let s = core::Scopes::shown(&rgba, w, h, &c, co, samples);
    compare(t, &|| format!("case {i}: {w}x{h} crop {c:?} turns {turns} flip {flip} samples {samples}"), &o, &s);
    if !flip {
        let s = core::Scopes::of(&rgba, w, h, &c, turns, samples);
        let o = oracle::Scopes::shown(&rgba, w, h, &oc, ocrop::Orientation::new(turns, false), samples);
        compare(t, &|| format!("case {i}: of"), &o, &s);
    }

    // The peak of any histogram, the inner bins empty now and then.
    let mut oh = oracle::Histogram::default();
    for ch in [&mut oh.red, &mut oh.green, &mut oh.blue] {
        let sparse = r.chance(2);
        for v in ch.iter_mut() {
            *v = if sparse && !r.chance(50) { 0 } else { r.next() as u32 >> r.below(32) };
        }
        if r.chance(3) {
            ch[1..255].fill(0);
        }
    }
    let ch = core::Histogram { red: oh.red, green: oh.green, blue: oh.blue };
    t.eq(&|| format!("case {i}: peak"), oh.peak(), ch.peak());

    // A raster of any waveform, with counts up to u32::MAX.
    if i % 10 == 0 {
        let width = 1 + r.below(256) as usize;
        let mut bins = || -> Vec<u32> { (0..width * 256).map(|_| if r.chance(3) { 0 } else { (r.next() as u32) >> r.below(32) }).collect() };
        let (red, green, blue) = (bins(), bins(), bins());
        let ow = oracle::Waveform { width, red: red.clone(), green: green.clone(), blue: blue.clone(), samples: 0 };
        let cw = core::Waveform { width, red, green, blue, samples: 0 };
        let (a, b) = (ow.raster(), cw.raster());
        t.compared += a.len() as u64;
        t.eq(&|| format!("case {i}: raster width {width}"), a == b, true);
    }
}

#[test]
fn scopes_match_the_rust() {
    let seed = seed(0x3b_415);
    let n = cases(10_000);
    gate("histogram", seed, |t| {
        let mut r = Random(seed);
        for i in 0..n {
            case(&mut r, t, i);
        }
        // A 24 MP preview at the app's budget, every orientation.
        let (w, h) = (6000, 4000);
        let rgba = picture(&mut r, w, h);
        for turns in 0..4 {
            for flip in [false, true] {
                let c = crop(&mut r);
                let o = oracle::Scopes::shown(&rgba, w, h, &o_crop(&c), ocrop::Orientation::new(turns, flip), 1 << 18);
                let s = core::Scopes::shown(&rgba, w, h, &c, ccrop::Orientation::new(turns, flip), 1 << 18);
                compare(t, &|| format!("24 MP turns {turns} flip {flip}"), &o, &s);
            }
        }
        n
    });
}
