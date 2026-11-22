//! What Phase 3a's differential tests (sheet.rs, tray.rs, compare.rs)
//! share: seeded inputs, a tally of differences, and the core's leak check.
//!
//! NEGS_RULES_SEEDS sets the number of seeded cases (default 10,000 per
//! module); NEGS_RULES_SEED the first seed.

#![allow(dead_code)]

use std::fmt::Debug;

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
        if n == 0 { 0 } else { self.next() % n }
    }
    pub fn index(&mut self, n: usize) -> usize {
        self.below(n as u64) as usize
    }
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
    pub fn bool(&mut self) -> bool {
        self.next() & 1 == 1
    }
    pub fn between(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }
    /// An awkward float: zeros of both signs, NaN, infinities, tiny, huge,
    /// halves (rounding's ties), or any bit pattern.
    pub fn edge(&mut self) -> f64 {
        match self.below(12) {
            0 => 0.0,
            1 => -0.0,
            2 => f64::NAN,
            3 => f64::INFINITY,
            4 => f64::NEG_INFINITY,
            5 => self.unit() * 1e-300,
            6 => (self.unit() - 0.5) * 1e300,
            7 => (self.below(4000) as f64 - 2000.0) + 0.5,
            8 => f64::MIN_POSITIVE,
            9 => -self.unit() * 100.0,
            10 => f64::from_bits(self.next()),
            _ => self.below(3000) as f64,
        }
    }
    /// A value in `low..high` mostly, now and then an awkward one.
    pub fn float(&mut self, low: f64, high: f64) -> f64 {
        if self.chance(12) { self.edge() } else { self.between(low, high) }
    }
    /// A whole number of DIPs in `low..high` mostly (the UI's usual inputs).
    pub fn whole(&mut self, low: f64, high: f64) -> f64 {
        if self.chance(3) { self.float(low, high) } else { self.between(low, high).round() }
    }
}

/// The number of seeded cases.
pub fn cases() -> u64 {
    std::env::var("NEGS_RULES_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(10_000)
}

/// The first seed.
pub fn first_seed() -> u64 {
    std::env::var("NEGS_RULES_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1)
}

/// A float's bits, NaNs all alike (both sides' NaNs come from the same
/// operations, but a NaN's payload isn't a value).
pub fn b(x: f64) -> u64 {
    if x.is_nan() { 0x7ff8_0000_0000_0000 } else { x.to_bits() }
}

pub fn bs<const N: usize>(x: [f64; N]) -> [u64; N] {
    x.map(b)
}

/// Every comparison, and the differences found.
#[derive(Default)]
pub struct Tally {
    pub compared: u64,
    pub differences: u64,
    pub first: Vec<String>,
    pub by_what: std::collections::BTreeMap<&'static str, u64>,
}

impl Tally {
    pub fn check<T: PartialEq + Debug>(&mut self, what: &'static str, seed: u64, oracle: T, core: T) {
        self.compared += 1;
        *self.by_what.entry(what).or_default() += 1;
        if oracle != core {
            self.differences += 1;
            if self.first.len() < 12 {
                self.first.push(format!("seed {seed}, {what}: oracle {oracle:?}, core {core:?}"));
            }
        }
    }

    /// Fails on any difference; reports what was compared.
    pub fn finish(&self, module: &str, cases: u64) {
        eprintln!("{module}: {cases} seeded cases, {} comparisons over {} functions, {} differences", self.compared, self.by_what.len(), self.differences);
        assert!(self.differences == 0, "{module}: {} differences:\n{}", self.differences, self.first.join("\n"));
    }
}

/// The core's allocations before a test, to check after it that nothing it
/// allocated is left (a `track` build; otherwise nothing to check).
pub struct Leaks(negs_core_sys::negs_alloc_stats_t);

impl Leaks {
    pub fn start() -> Leaks {
        Leaks(negs_core_sys::alloc_stats())
    }

    pub fn check(&self, module: &str) {
        let after = negs_core_sys::alloc_stats();
        if after.tracked == 1 {
            eprintln!("{module}: core allocations {} (live before {}, after {}), bad frees {}", after.total_count - self.0.total_count, self.0.live_count, after.live_count, after.bad_frees);
            assert_eq!(after.live_count, self.0.live_count, "{module}: the core leaked");
            assert_eq!(after.live_bytes, self.0.live_bytes, "{module}: the core leaked");
            assert_eq!(after.bad_frees, 0, "{module}: bad frees in the core");
        }
    }
}
