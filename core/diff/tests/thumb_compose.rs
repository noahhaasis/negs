//! The edited thumbnail, windows/src/thumb_compose.rs as it is now (its
//! reduced size and Rec. 2020 to sRGB on the core) compiled against the
//! rules crate, against the Rust oracle (windows/src/thumb_compose.rs as it
//! was, core/diff/oracle): `from_preview` on seeded previews, crops,
//! turns, borders and wide or sRGB primaries (NEGS_RULES_SEEDS, default
//! 10,000 sizes and a quarter as many thumbnails, NEGS_RULES_SEED; a fifth
//! of them carry a border, drawn by the engine from data/), every byte of
//! the thumbnail compared. The rules
//! alone too: the reduced size at any size and share, and to_srgb on every
//! level of every channel. windows/src's own tests run here as well, on
//! the core.

mod picture_common;

use negs_core_diff_picture::{border as oborder, crop as ocrop, thumb_compose as oracle};
use negs_core_rules::thumb_compose as rules;
use picture_common::{Random, Tally, cases, gate, seed};
use std::path::PathBuf;

// The windows crate's modules thumb_compose.rs names, on the rules crate.
mod border {
    pub use negs_core_rules::border::*;
}
mod crop {
    pub use negs_core_rules::crop::*;
}
mod gpu {
    pub use negs_core_diff_picture::gpu::*;
}
mod state {
    /// What `Finish::of` reads.
    pub struct EditState {
        pub border: crate::border::BorderSettings,
        pub film: String,
    }
    impl EditState {
        pub fn rotation(&self) -> i32 {
            0
        }
        pub fn flip(&self) -> bool {
            self.film.is_empty()
        }
    }
}
#[path = "../../../windows/src/thumb_compose.rs"]
mod app;

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn o_crop(c: &crop::CropSettings) -> ocrop::CropSettings {
    ocrop::CropSettings {
        rect: ocrop::Rect::new(c.rect.x, c.rect.y, c.rect.width, c.rect.height),
        aspect: ocrop::CropAspect::ALL[crop::CropAspect::ALL.iter().position(|a| *a == c.aspect).unwrap()],
        custom: ocrop::Size::new(c.custom.width, c.custom.height),
        angle: c.angle,
    }
}

fn o_border(b: &border::BorderSettings) -> oborder::BorderSettings {
    oborder::BorderSettings {
        enabled: b.enabled,
        style: b.style,
        export: b.export,
        instagram: b.instagram,
        width: b.width,
        film_width: b.film_width,
        line: b.line,
        color: oborder::BorderColor(b.color.0),
        line_color: oborder::BorderColor(b.line_color.0),
    }
}

fn settings(r: &mut Random) -> crop::CropSettings {
    if r.chance(3) {
        return crop::CropSettings::default();
    }
    let rect = match r.below(6) {
        // Fractions just outside the frame too; not NaN or 1e300, which the
        // engine's crop (the same on both sides) can't allocate for.
        0 => crop::Rect::new(r.range(-0.1, 1.0), r.range(-0.1, 1.0), r.range(0.0, 1.2), r.range(0.0, 1.2)),
        _ => {
            let (x, y) = (r.range(0.0, 0.8), r.range(0.0, 0.8));
            crop::Rect::new(x, y, r.range(0.02, 1.0 - x), r.range(0.02, 1.0 - y))
        }
    };
    let angle = if r.chance(2) { 0.0 } else { r.range(-45.0, 45.0) };
    crop::CropSettings { rect, angle, ..crop::CropSettings::default() }
}

fn border(r: &mut Random) -> border::BorderSettings {
    border::BorderSettings {
        enabled: true,
        style: border::STYLES[r.below(4) as usize],
        instagram: r.chance(3),
        width: r.range(0.0, 0.1),
        film_width: r.range(0.0, 0.05),
        line: r.range(0.0, 0.02),
        color: border::BorderColor([r.next() as u8, r.next() as u8, r.next() as u8]),
        ..border::BorderSettings::default()
    }
}

fn preview(r: &mut Random, w: u32, h: u32) -> Vec<u8> {
    let n = w as usize * h as usize * 4;
    if r.chance(2) {
        (0..n).map(|_| r.next() as u8).collect()
    } else {
        (0..n).map(|i| ((i / 4) as u32 % w * 255 / w.max(1)) as u8 ^ ((i % 4) as u8 * 60)).collect()
    }
}

/// A thumbnail from a seeded preview, both ways.
fn thumbnail(r: &mut Random, t: &mut Tally, i: usize) {
    let (w, h) = match r.below(6) {
        0 => (1 + r.below(4) as u32, 1 + r.below(4) as u32),
        1 => (600 + r.below(1400) as u32, 400 + r.below(1000) as u32),
        _ => (1 + r.below(260) as u32, 1 + r.below(260) as u32),
    };
    let mut pixels = preview(r, w, h);
    if r.chance(50) {
        pixels.truncate(pixels.len() - 1);
    }
    let wide = r.chance(2);
    let c = settings(r);
    let long_side = match r.below(4) {
        0 => 1 + r.below(16) as u32,
        1 => 512,
        _ => 32 + r.below(300) as u32,
    };
    let (rotation, flip) = (r.int(-5, 5) as i32, r.chance(2));
    let bordered = r.chance(5);
    let b = border(r);
    // A film the 35mm strip knows, or one it doesn't (the generic strip).
    let film = if r.chance(2) { "kodak_portra_400" } else { "not_a_film" }.to_string();
    let ofinish = oracle::Finish { rotation, flip, border: bordered.then(|| o_border(&b)), film: film.clone(), data: bordered.then(data) };
    let cfinish = app::Finish { rotation, flip, border: bordered.then_some(b), film, data: bordered.then(data) };
    if i == 0 {
        let s = state::EditState { border: b, film: cfinish.film.clone() };
        t.eq(&|| "Finish::of".into(), app::Finish::of(&s, None).border.is_some(), true);
    }
    let o = oracle::from_preview(&pixels, w, h, wide, &o_crop(&c), &ofinish, long_side);
    let a = app::from_preview(&pixels, w, h, wide, &c, &cfinish, long_side);
    let what = || format!("case {i}: {w}x{h} wide {wide} crop {c:?} long side {long_side} turns {rotation} flip {flip} border {bordered}");
    t.eq(&what, o.as_ref().map(|o| o.dimensions()), a.as_ref().map(|a| a.dimensions()));
    if let (Some(o), Some(a)) = (o, a) {
        t.compared += o.as_raw().len() as u64;
        if o.as_raw() != a.as_raw() {
            t.eq(&what, "pixels differ", "");
        }
    }
}

/// The size rule alone, at sizes and shares the previews don't reach,
/// against the oracle's from_preview's first lines restated (the
/// thumbnails hold the rule to the oracle itself).
fn size(r: &mut Random, t: &mut Tally, i: usize) {
    let (cw, ch) = (r.fraction(), r.fraction());
    let (bw, bh) = (1 + r.below(9000) as u32, 1 + r.below(9000) as u32);
    let ls = 1 + r.below(2048) as u32;
    let share = cw.max(ch).clamp(0.05, 1.0);
    let wanted = (2.0 * ls as f64 / share).ceil() as u32;
    let longest = bw.max(bh);
    let expected = (longest > wanted).then(|| {
        let scale = wanted as f64 / longest as f64;
        (((bw as f64 * scale).round() as u32).max(1), ((bh as f64 * scale).round() as u32).max(1))
    });
    t.eq(&|| format!("case {i}: reduced_size {bw}x{bh} share {cw}x{ch} long side {ls}"), expected, rules::reduced_size(bw, bh, (cw, ch), ls));
}

#[test]
fn thumbnails_match_the_rust() {
    let seed = seed(0x3b_7c0);
    let n = cases(10_000);
    gate("thumb_compose", seed, |t| {
        let mut r = Random(seed);
        // Every case a size; every fourth a thumbnail too.
        for i in 0..n {
            size(&mut r, t, i);
            if i % 4 == 0 {
                thumbnail(&mut r, t, i);
            }
        }
        // to_srgb on every level of each channel, and on noise: a wide
        // 256 × 3 preview at its own size goes through to_srgb alone.
        let mut levels: Vec<u8> = Vec::new();
        for c in 0..3 {
            for v in 0..=255u8 {
                let mut p = [0u8, 0, 0, 255];
                p[c] = v;
                levels.extend(p);
            }
        }
        levels.extend((0..4096).map(|_| r.next() as u8));
        let count = levels.len() as u32 / 4;
        let plain = oracle::Finish { rotation: 0, flip: false, border: None, film: String::new(), data: None };
        let o = oracle::from_preview(&levels, count, 1, true, &ocrop::CropSettings::default(), &plain, 1 << 20).unwrap();
        let mut a = levels.clone();
        rules::to_srgb(&mut a, &negs_core_diff_picture::gpu::REC2020_TO_SRGB);
        t.compared += a.len() as u64;
        t.eq(&|| "to_srgb levels".into(), o.as_raw() == &a, true);
        n
    });
}
