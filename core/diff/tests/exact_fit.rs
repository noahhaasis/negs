//! The exact fit, Odin core (negs_core_rules::exact_fit) against the Rust
//! oracle (windows/src/exact_fit.rs as it was, core/diff/oracle). The sizes
//! and Pillow's coefficients on seeded inputs (NEGS_RULES_SEEDS, default
//! 10,000; NEGS_RULES_SEED), every weight bit for bit; the resampled
//! pixels byte for byte on seeded pictures of many sizes (a fiftieth as
//! many: shrinking, growing, one pixel wide, noise and smooth gradients) and
//! on the fitted view's sizes, 6000×4000 to 1500×1000 among them.

mod picture_common;

use negs_core_diff::exact_fit as oracle;
use negs_core_rules::exact_fit as core;
use picture_common::{Random, Tally, cases, gate, seed};

fn picture(r: &mut Random, w: u32, h: u32) -> Vec<u8> {
    let n = w as usize * h as usize * 4;
    match r.below(4) {
        // Noise: every level, the worst case for the sums.
        0 => (0..n).map(|_| r.next() as u8).collect(),
        // A smooth gradient with a little noise, like a photograph.
        1 => {
            let (a, b) = (r.range(0.0, 0.2), r.range(0.0, 0.2));
            (0..n)
                .map(|i| {
                    let p = i / 4;
                    let (x, y) = ((p % w as usize) as f64, (p / w as usize) as f64);
                    ((x * a + y * b + (i % 4) as f64 * 40.0) as u64 % 256) as u8 ^ (r.next() as u8 & 3)
                })
                .collect()
        }
        // Hard edges: stripes and checks.
        2 => {
            let k = 1 + r.below(7) as usize;
            (0..n).map(|i| { let p = i / 4; if ((p % w as usize) / k + (p / w as usize) / k) % 2 == 0 { 255 } else { 0 } }).collect()
        }
        // The bench's pattern.
        _ => (0..n).map(|i| (i * 31 % 251) as u8).collect(),
    }
}

fn sizes(r: &mut Random, t: &mut Tally, i: usize) {
    let w = |what: &'static str| move || format!("case {i}: {what}");
    let (pw, ph) = (r.length(), r.length());
    let z = match r.below(6) {
        0 => r.wild(),
        1 => 1.0,
        2 => 0.999 - r.unit() * 1e-3,
        _ => r.range(0.0, 1.2),
    };
    t.eq(&w("target"), oracle::target(pw, ph, z), core::target(pw, ph, z));
    let texture = (r.pixels(), r.pixels());
    let target = if r.chance(2) { (r.pixels(), r.pixels()) } else { (texture.0.saturating_sub(r.below(3) as u32), texture.1.saturating_sub(r.below(3) as u32)) };
    t.eq(&w("worth_it"), oracle::worth_it(z, texture, target), core::worth_it(z, texture, target));
    // Pillow's coefficients: shrinking mostly, by any factor, growing too.
    let input = match r.below(6) {
        0 => 1 + r.below(4) as u32,
        1 => 6000,
        _ => 1 + r.below(9000) as u32,
    };
    let output = match r.below(6) {
        0 => 1 + r.below(4) as u32,
        1 => input,
        2 => 1 + r.below(input as u64 * 3) as u32,
        _ => 1 + r.below(input as u64) as u32,
    };
    // Every row at small sizes; a sample of a hundred otherwise, so 10,000
    // cases stay quick (the resampling below compares every row it uses).
    let (o, c) = (oracle::coefficients(input, output), core::coefficients(input, output));
    t.eq(&w("coefficient rows"), o.len(), c.len());
    let step = (o.len() / 100).max(1);
    for (k, (a, b)) in o.iter().zip(&c).enumerate().step_by(step) {
        t.eq(&|| format!("case {i}: coefficients {input}->{output} row {k} start"), a.0, b.0);
        t.eq(&|| format!("case {i}: coefficients {input}->{output} row {k} count"), a.1.len(), b.1.len());
        for (j, (p, q)) in a.1.iter().zip(&b.1).enumerate() {
            t.compared += 1;
            if p.to_bits() != q.to_bits() {
                t.eq(&|| format!("case {i}: coefficients {input}->{output} row {k} weight {j}"), p.to_bits(), q.to_bits());
            }
        }
    }
}

fn pixels(t: &mut Tally, what: String, rgba: &[u8], w: u32, h: u32, tw: u32, th: u32) {
    let (a, b) = (oracle::resample(rgba, w, h, tw, th), core::resample(rgba, w, h, tw, th));
    t.compared += a.len() as u64;
    if a.len() != b.len() {
        t.eq(&|| format!("{what}: length"), a.len(), b.len());
        return;
    }
    let differing = a.iter().zip(&b).filter(|(p, q)| p != q).count();
    if differing > 0 {
        let first = a.iter().zip(&b).position(|(p, q)| p != q).unwrap();
        let largest = a.iter().zip(&b).map(|(p, q)| p.abs_diff(*q)).max().unwrap();
        t.differing += differing as u64 - 1;
        t.eq(&|| format!("{what}: {differing} bytes differ (largest {largest}), first at {first}"), a[first], b[first]);
    }
}

fn resample(r: &mut Random, t: &mut Tally, i: usize) {
    let (w, h) = match r.below(5) {
        0 => (1 + r.below(4) as u32, 1 + r.below(300) as u32),
        1 => (1 + r.below(300) as u32, 1 + r.below(4) as u32),
        _ => (1 + r.below(400) as u32, 1 + r.below(400) as u32),
    };
    let (tw, th) = match r.below(5) {
        0 => (w, h),
        1 => (1 + r.below(w as u64 * 2) as u32, 1 + r.below(h as u64 * 2) as u32),
        2 => (1, 1 + r.below(h as u64) as u32),
        _ => (1 + r.below(w as u64) as u32, 1 + r.below(h as u64) as u32),
    };
    let rgba = picture(r, w, h);
    pixels(t, format!("resample {i}: {w}x{h} -> {tw}x{th}"), &rgba, w, h, tw, th);
}

#[test]
fn exact_fit_matches_the_rust() {
    let seed = seed(0x3b_f17);
    let n = cases(10_000);
    gate("exact_fit", seed, |t| {
        let mut r = Random(seed);
        for i in 0..n {
            sizes(&mut r, t, i);
            if i % 50 == 0 {
                resample(&mut r, t, i);
            }
        }
        // The fitted view's sizes: a 24 MP photograph at a laptop's and a
        // large display's fit, a portrait one, Limit preview's 4896 × 3264.
        for (w, h, tw, th) in [(6000, 4000, 1500, 1000), (6000, 4000, 2200, 1467), (4000, 6000, 1035, 1553), (4896, 3264, 1553, 1035), (3000, 2000, 2999, 1999), (2000, 3000, 7, 3000)] {
            let rgba = picture(&mut r, w, h);
            pixels(t, format!("{w}x{h} -> {tw}x{th}"), &rgba, w, h, tw, th);
        }
        n
    });
}
