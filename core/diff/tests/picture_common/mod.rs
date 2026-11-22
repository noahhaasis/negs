//! Shared by Phase 3b's differential tests (crop, border, exact_fit,
//! thumb_compose, histogram, negative): seeded inputs, a tally of every
//! value compared bit for bit, and the core's leak check.
#![allow(dead_code)]

/// SplitMix64, as the library's state tests.
pub struct Random(pub u64);

impl Random {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    pub fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    pub fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
    /// A fraction of a photograph, with the edges a crop meets: 0, 1, -0,
    /// just outside, round thousandths, and now and then something wild.
    pub fn fraction(&mut self) -> f64 {
        match self.below(24) {
            0 => 0.0,
            1 => 1.0,
            2 => -0.0,
            3 => 0.5,
            4 => -self.unit() * 0.3,
            5 => 1.0 + self.unit() * 0.3,
            6 => (self.below(1001) as f64) / 1000.0,
            7 => 1e-4 * self.unit(),
            8 => 1.0 - 1e-4 * self.unit(),
            9 => self.wild(),
            _ => self.unit(),
        }
    }
    /// A length in pixels (or an angle's worth), with the edges.
    pub fn length(&mut self) -> f64 {
        match self.below(20) {
            0 => 0.0,
            1 => -self.unit() * 50.0,
            2 => self.unit() * 1e-3,
            3 => 1e5 + self.unit() * 1e6,
            4 => self.below(8000) as f64,
            5 => self.wild(),
            _ => 1.0 + self.unit() * 6000.0,
        }
    }
    /// A straightening angle: mostly within ±45° and in tenths, sometimes 0
    /// or beyond.
    pub fn angle(&mut self) -> f64 {
        match self.below(10) {
            0 | 1 => 0.0,
            2 => (self.int(-450, 450) as f64) / 10.0,
            3 => self.range(-360.0, 360.0),
            4 => -0.0,
            5 => 90.0 * self.int(-4, 4) as f64,
            _ => self.range(-45.0, 45.0),
        }
    }
    /// NaN, infinities, signed zeros, subnormals, extremes.
    pub fn wild(&mut self) -> f64 {
        match self.below(9) {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => f64::NEG_INFINITY,
            3 => -0.0,
            4 => f64::MIN_POSITIVE / 2.0,
            5 => f64::MAX,
            6 => -f64::MAX,
            7 => f64::from_bits(self.next()),
            _ => 4294967295.5,
        }
    }
    /// A pixel count, with 0, 1 and large.
    pub fn pixels(&mut self) -> u32 {
        match self.below(16) {
            0 => 0,
            1 => 1,
            2 => self.below(4) as u32,
            3 => u32::MAX - self.below(3) as u32,
            4 => 10_000 + self.below(60_000) as u32,
            _ => 1 + self.below(8000) as u32,
        }
    }
}

/// How many seeded cases: NEGS_RULES_SEEDS, else `default`.
pub fn cases(default: usize) -> usize {
    std::env::var("NEGS_RULES_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// The seed: NEGS_RULES_SEED, else `default`.
pub fn seed(default: u64) -> u64 {
    std::env::var("NEGS_RULES_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Every value compared; floats bit for bit (any NaN equals any NaN: Rust
/// doesn't promise NaN payloads either).
#[derive(Default)]
pub struct Tally {
    pub compared: u64,
    pub differing: u64,
    pub first: Vec<String>,
}

impl Tally {
    fn miss(&mut self, what: &dyn Fn() -> String, detail: String) {
        self.differing += 1;
        if self.first.len() < 8 {
            self.first.push(format!("{}: {detail}", what()));
        }
    }
    pub fn f(&mut self, what: &dyn Fn() -> String, oracle: f64, core: f64) {
        self.compared += 1;
        if oracle.to_bits() != core.to_bits() && !(oracle.is_nan() && core.is_nan()) {
            self.miss(what, format!("oracle {oracle:?} core {core:?}"));
        }
    }
    pub fn eq<T: PartialEq + std::fmt::Debug>(&mut self, what: &dyn Fn() -> String, oracle: T, core: T) {
        self.compared += 1;
        if oracle != core {
            self.miss(what, format!("oracle {oracle:?} core {core:?}"));
        }
    }
    /// Values whose Debug form shows every float exactly ({:?} of an f64 is
    /// its shortest round trip), so equal text is equal bits, NaN aside.
    pub fn dbg<T: std::fmt::Debug>(&mut self, what: &dyn Fn() -> String, oracle: T, core: T) {
        self.compared += 1;
        let (a, b) = (format!("{oracle:?}"), format!("{core:?}"));
        if a != b {
            self.miss(what, format!("oracle {a} core {b}"));
        }
    }
}

fn gate_with(name: &str, seed: u64, shared: bool, f: impl FnOnce(&mut Tally) -> usize) {
    let before = negs_core_sys::alloc_stats();
    let mut t = Tally::default();
    let n = f(&mut t);
    let after = negs_core_sys::alloc_stats();
    println!(
        "{name} differential: {n} cases (seed {seed:#x}), {} values compared, {} differ; core allocations {} live before, {} after, {} made, {} bad frees",
        t.compared,
        t.differing,
        before.live_count,
        after.live_count,
        after.total_count - before.total_count,
        after.bad_frees
    );
    assert_eq!(t.differing, 0, "{name}: first differences:\n{}", t.first.join("\n"));
    if after.tracked == 1 {
        if shared {
            println!("  (live counts include the other tests running beside it: not compared)");
        } else {
            assert_eq!(after.live_count, before.live_count, "{name}: the core leaked");
        }
        assert_eq!(after.bad_frees, 0, "{name}: bad frees");
    }
}

/// Runs `f` and checks the core's allocations: none left behind, no bad
/// frees (when the core counts them: `--features track`). Prints the tally.
pub fn gate(name: &str, seed: u64, f: impl FnOnce(&mut Tally) -> usize) {
    gate_with(name, seed, false, f)
}

/// `gate` for a test file with other tests in it, which run on other
/// threads at the same time: the core's live count is the process's, so
/// only differences and bad frees are checked.
pub fn gate_shared(name: &str, seed: u64, f: impl FnOnce(&mut Tally) -> usize) {
    gate_with(name, seed, true, f)
}
