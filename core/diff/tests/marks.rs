//! The marks, Odin core against the Rust oracle (windows/marks): 10,000
//! seeded inputs (names, sizes, kinds), every coordinate, count and kind
//! compared. The plan's gate is 1e-12; on Apple silicon they match bit for
//! bit, and must (NEGS_MARKS_EXACT=0 relaxes it, =1 demands it elsewhere).
//! On x86_64 Rust's LLVM folds some sines of constant angles at compile time
//! with the build machine's libm, so the last bit can differ (~3e-14 seen).
//! NEGS_MARKS_CASES overrides the count, NEGS_MARKS_SEED the seed.

use negs_core_marks as core;
use negs_marks as oracle;

/// SplitMix64, as the library's state tests.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// A size a front end might ask for, with the edges: 0, negative, tiny, huge.
    fn size(&mut self) -> f64 {
        match self.below(20) {
            0 => 0.0,
            1 => -self.unit() * 50.0,
            2 => self.unit() * 1e-3,
            3 => 1e5 + self.unit() * 1e6,
            _ => 1.0 + self.unit() * 1200.0,
        }
    }
}

const STEMS: [&str; 10] = ["DSC_", "IMG_", "_NHA", "Ålesund ", "東京 ", "📷 ", "f", "", "Łódź—", "a\u{0301}"];
const EXTS: [&str; 6] = [".NEF", ".JPG", ".dng", ".CR3", "", ".tiff"];

fn name(r: &mut Random) -> String {
    let stem = STEMS[r.below(STEMS.len() as u64) as usize];
    let ext = EXTS[r.below(EXTS.len() as u64) as usize];
    let mut name = format!("{stem}{}{ext}", r.below(100_000));
    if r.below(30) == 0 {
        name = name.repeat(1 + r.below(8) as usize);
    }
    if r.below(25) == 0 {
        name.push_str(" (2)");
    }
    name
}

/// Bit-for-bit equality, and the largest difference seen.
#[derive(Default)]
struct Tally {
    compared: u64,
    differing: u64,
    largest: f64,
    first: Option<String>,
}

impl Tally {
    fn f(&mut self, what: &dyn Fn() -> String, a: f64, b: f64) {
        self.compared += 1;
        if a.to_bits() != b.to_bits() && !(a.is_nan() && b.is_nan()) {
            self.differing += 1;
            let d = (a - b).abs();
            if d > self.largest || d.is_nan() {
                self.largest = d;
            }
            if self.first.is_none() {
                self.first = Some(format!("{}: oracle {a:?} core {b:?}", what()));
            }
        }
    }
    fn pt(&mut self, what: &dyn Fn() -> String, a: oracle::Pt, b: core::Pt) {
        self.f(what, a.x, b.x);
        self.f(what, a.y, b.y);
    }
    fn count(&mut self, what: &dyn Fn() -> String, a: usize, b: usize) {
        self.compared += 1;
        if a != b {
            self.differing += 1;
            self.largest = f64::INFINITY;
            if self.first.is_none() {
                self.first = Some(format!("{}: oracle {a} core {b}", what()));
            }
        }
    }
    fn outlines(&mut self, what: &dyn Fn() -> String, a: &[Vec<oracle::Pt>], b: &[Vec<core::Pt>]) {
        self.count(what, a.len(), b.len());
        for (x, y) in a.iter().zip(b) {
            self.count(what, x.len(), y.len());
            for (&p, &q) in x.iter().zip(y) {
                self.pt(what, p, q);
            }
        }
    }
    fn path(&mut self, what: &dyn Fn() -> String, a: &oracle::Path, b: &core::Path) {
        self.count(what, a.0.len(), b.0.len());
        for (x, y) in a.0.iter().zip(&b.0) {
            let (xs, ys) = (seg_points(x), seg_points_core(y));
            self.count(what, xs.len(), ys.len());
            for (&p, &q) in xs.iter().zip(&ys) {
                self.pt(what, p, q.core());
            }
        }
        match (a.bounds(), b.bounds()) {
            (Some(p), Some(q)) => {
                for (u, v) in [(p.x, q.x), (p.y, q.y), (p.w, q.w), (p.h, q.h)] {
                    self.f(what, u, v);
                }
            }
            (p, q) => self.count(what, p.is_some() as usize, q.is_some() as usize),
        }
    }
}

fn seg_points(s: &oracle::Seg) -> Vec<oracle::Pt> {
    match *s {
        oracle::Seg::Move(p) | oracle::Seg::Line(p) => vec![p],
        oracle::Seg::Cubic(a, b, c) => vec![a, b, c, oracle::pt(f64::NAN, 2.0)],
        oracle::Seg::Close => vec![oracle::pt(f64::NAN, 3.0)],
    }
}

fn seg_points_core(s: &core::Seg) -> Vec<oracle::Pt> {
    match *s {
        core::Seg::Move(p) | core::Seg::Line(p) => vec![oracle::pt(p.x, p.y)],
        core::Seg::Cubic(a, b, c) => vec![oracle::pt(a.x, a.y), oracle::pt(b.x, b.y), oracle::pt(c.x, c.y), oracle::pt(f64::NAN, 2.0)],
        core::Seg::Close => vec![oracle::pt(f64::NAN, 3.0)],
    }
}

trait AsCore {
    fn core(self) -> core::Pt;
}
impl AsCore for oracle::Pt {
    fn core(self) -> core::Pt {
        core::pt(self.x, self.y)
    }
}

#[test]
fn ten_thousand_cases_match_bit_for_bit() {
    let cases: u64 = std::env::var("NEGS_MARKS_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(10_000);
    let seed: u64 = std::env::var("NEGS_MARKS_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0x6e65_6773);
    let before = negs_core_sys_stats();
    let mut r = Random(seed);
    let mut t = Tally::default();
    let mut kinds = [0u64; 9];
    let mut differing_by_kind = [0u64; 9];
    for case in 0..cases {
        let name = name(&mut r);
        let (w, h) = (r.size(), r.size());
        let kind = r.below(9) as usize;
        kinds[kind] += 1;
        let differing_before = t.differing;
        let what = || format!("case {case} kind {kind} {name:?} {w}×{h}");
        t.count(&what, oracle::hash(&name) as usize, core::hash(&name) as usize);
        match kind {
            0 => {
                let rect = (r.size() - 600.0, r.size() - 600.0);
                let plain = r.below(2) == 0;
                t.path(&what, &oracle::pencil(oracle::Rect::new(rect.0, rect.1, w, h), plain), &core::pencil(core::Rect::new(rect.0, rect.1, w, h), plain));
            }
            1 => {
                let plain = r.below(3) == 0;
                let (o, c) = match r.below(3) {
                    0 => (oracle::Radii::BOX, core::Radii::BOX),
                    1 => (oracle::Radii::NUMBER, core::Radii::NUMBER),
                    _ => {
                        let relative = r.below(2) == 0;
                        let scale = if relative { 1.0 } else { 60.0 };
                        let hz = [r.unit() * scale, r.unit() * scale, r.unit() * scale, r.unit() * scale];
                        let vt = [r.unit() * scale, r.unit() * scale, r.unit() * scale, r.unit() * scale];
                        (oracle::Radii { horizontal: hz, vertical: vt, relative }, core::Radii { horizontal: hz, vertical: vt, relative })
                    }
                };
                let rect = (r.unit() * 100.0, r.unit() * 100.0);
                t.path(&what, &oracle::ring(oracle::Rect::new(rect.0, rect.1, w, h), o, plain), &core::ring(core::Rect::new(rect.0, rect.1, w, h), c, plain));
                let deg = r.unit() * 4.0 - 2.0;
                t.f(&what, oracle::ring_tilt(deg, plain), core::ring_tilt(deg, plain));
            }
            2 | 3 => {
                let index = r.below(12) as usize;
                t.count(&what, oracle::star_seed(&name, index) as usize, core::star_seed(&name, index) as usize);
                t.outlines(&what, &oracle::star_strokes(&name, index), &core::star_strokes(&name, index));
                if kind == 3 {
                    t.path(&what, &oracle::star_path(&name, index), &core::star_path(&name, index));
                }
            }
            4 => {
                t.count(&what, oracle::cross_kind(&name) as usize, core::cross_kind(&name) as usize);
                let (a, b) = (oracle::cross_strokes(w, h, &name), core::cross_strokes(w, h, &name));
                t.count(&what, a.len(), b.len());
                for (x, y) in a.iter().zip(&b) {
                    t.f(&what, x.opacity, y.opacity);
                }
                let a: Vec<_> = a.into_iter().map(|s| s.outline).collect();
                let b: Vec<_> = b.into_iter().map(|s| s.outline).collect();
                t.outlines(&what, &a, &b);
            }
            _ => {
                let (a, b) = match kind {
                    5 => (oracle::pick_box_grid(w, h, &name), core::pick_box_grid(w, h, &name)),
                    6 => (oracle::pick_box_strip(w, h, &name), core::pick_box_strip(w, h, &name)),
                    7 => (oracle::pick_box_selection(w, h, &name), core::pick_box_selection(w, h, &name)),
                    _ => {
                        let o = r.unit() * 12.0;
                        let tight = r.below(2) == 0;
                        let weight = 0.5 + r.unit() * 2.0;
                        (oracle::pick_box(w, h, o, &name, tight, weight), core::pick_box(w, h, o, &name, tight, weight))
                    }
                };
                t.count(&what, a.kind as usize, b.kind as usize);
                t.count(&what, oracle::box_kind(&name) as usize, core::box_kind(&name) as usize);
                t.f(&what, a.tilt, b.tilt);
                t.outlines(&what, &a.strokes, &b.strokes);
            }
        }
        differing_by_kind[kind] += t.differing - differing_before;
        // The polygons' bounds, through the core's path bounds.
        if case % 97 == 0 {
            let pts: Vec<oracle::Pt> = (0..1 + r.below(40)).map(|_| oracle::pt(r.unit() * w, r.unit() * h)).collect();
            let core_pts: Vec<core::Pt> = pts.iter().map(|&p| p.core()).collect();
            t.path(&what, &oracle::Path::polygon(&pts), &core::Path::polygon(&core_pts));
        }
    }
    let after = negs_core_sys_stats();
    println!(
        "marks differential: {cases} cases (seed {seed:#x}, kinds {kinds:?}), {} values compared, {} differ (by kind {differing_by_kind:?}, largest {:e}); core allocations {} live before, {} after, {} made, {} bad frees",
        t.compared, t.differing, t.largest, before.live_count, after.live_count, after.total_count - before.total_count, after.bad_frees
    );
    let exact = match std::env::var("NEGS_MARKS_EXACT").as_deref() {
        Ok("0") => false,
        Ok(_) => true,
        Err(_) => cfg!(all(target_arch = "aarch64", target_os = "macos")),
    };
    assert!(t.largest <= 1e-12, "beyond 1e-12: {}", t.first.clone().unwrap_or_default());
    if exact {
        assert_eq!(t.differing, 0, "first difference: {}", t.first.unwrap_or_default());
    }
    if after.tracked == 1 {
        assert_eq!(after.live_count, before.live_count, "the core leaked");
        assert_eq!(after.bad_frees, 0);
    }
}

fn negs_core_sys_stats() -> negs_core_sys::negs_alloc_stats_t {
    negs_core_sys::alloc_stats()
}
