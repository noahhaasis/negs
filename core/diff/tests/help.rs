//! Tooltips, Odin core against the frozen Rust original
//! (core/diff/oracle/help.rs): every key of both tables, each with seeded
//! changes (cut, extended, recased, a byte changed, a lane index added:
//! valid, signed, overflowing, empty, nested), every grain field's lanes,
//! the backlight's path, and the constants. NEGS_RULES_SEEDS seeds (default
//! 100, 100 cases each).

use negs_core_rules::help as core;
use negs_media_oracle::help as oracle;
use negs_media_oracle::{negative, state};

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
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// The keys of one of the oracle's tables, from its source.
fn keys(source: &str, table: &str) -> Vec<String> {
    let start = source.find(&format!("const {table}: &[(&str, &str)] = &[")).unwrap();
    let body = &source[start..start + source[start..].find("\n];").unwrap()];
    body.lines().filter_map(|l| l.trim().strip_prefix("(\"")?.split_once("\", ").map(|(k, _)| k.to_string())).collect()
}

fn text(path: &str) -> Option<&'static str> {
    core::text(path, negative::SCAN_ILLUMINANT, &state::GRAIN_STOCK_FIELDS)
}

/// A key changed a little.
fn mutate(r: &mut Random, key: &str) -> String {
    let lanes = ["0", "1", "2", "17", "+3", "-1", "", "18446744073709551615", "18446744073709551616", "1.2", "x", "٣", " 1", "00"];
    let mut k = key.to_string();
    match r.below(9) {
        0 => k.truncate(r.below(k.len() as u64 + 1) as usize),
        1 => k.push_str(&format!(".{}", lanes[r.below(lanes.len() as u64) as usize])),
        2 => k.push_str([".", "x", "_", ".a.b", " "][r.below(5) as usize]),
        3 => k = k.to_uppercase(),
        4 if !k.is_empty() => {
            let mut b = k.into_bytes();
            let i = r.below(b.len() as u64) as usize;
            b[i] = b"._azAZ09"[r.below(8) as usize];
            k = String::from_utf8_lossy(&b).into_owned();
        }
        5 => k = format!("{}{k}", ["film_render.grain.", ".", "x", ""][r.below(4) as usize]),
        6 => k = format!("{k}\u{0}"),
        7 => {
            if let Some(i) = k.rfind('.') {
                k.truncate(i);
            }
        }
        _ => {}
    }
    k
}

#[test]
fn the_core_gives_the_same_tooltips() {
    let before = negs_core_sys::alloc_stats();
    let source = include_str!("../oracle/help.rs");
    let (paths, groups) = (keys(source, "PATHS"), keys(source, "GROUPS"));
    assert!(paths.len() > 50 && groups.len() > 5);
    assert_eq!(core::paths().iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>(), paths, "the paths' keys, in order");
    assert_eq!(core::groups().iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>(), groups, "the groups' keys, in order");
    assert_eq!(
        [core::SCAN_ILLUMINANT, core::FILM, core::PAPER, core::PAPER_LOCK, core::PAPER_KEPT],
        [oracle::SCAN_ILLUMINANT, oracle::FILM, oracle::PAPER, oracle::PAPER_LOCK, oracle::PAPER_KEPT]
    );
    assert_eq!(core::core_constants(), [oracle::SCAN_ILLUMINANT, oracle::FILM, oracle::PAPER, oracle::PAPER_LOCK, oracle::PAPER_KEPT]);

    let mut cases = 0u64;
    let mut found = 0u64;
    let same = |path: &str, cases: &mut u64, found: &mut u64| {
        let expected = oracle::text(path);
        assert_eq!(text(path), expected, "text({path:?})");
        let expected = oracle::group(path);
        assert_eq!(core::group(path), expected, "group({path:?})");
        *cases += 2;
        *found += oracle::text(path).is_some() as u64 + expected.is_some() as u64;
    };
    for key in paths.iter().chain(&groups) {
        same(key, &mut cases, &mut found);
    }
    same(negative::SCAN_ILLUMINANT, &mut cases, &mut found);
    for field in state::GRAIN_STOCK_FIELDS.iter().chain(&["active", "sublayers_active", "blur"]) {
        for lane in ["0", "1", "2", "3", "99", "", "+1", "-0", "1.0", "18446744073709551615", "18446744073709551616", "0x1"] {
            same(&format!("film_render.grain.{field}.{lane}"), &mut cases, &mut found);
        }
    }
    let seeds: u64 = std::env::var("NEGS_RULES_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(100);
    let all: Vec<&String> = paths.iter().chain(&groups).collect();
    for seed in 0..seeds {
        let mut r = Random(seed ^ 0x4e1b);
        for _ in 0..100 {
            let key = all[r.below(all.len() as u64) as usize];
            let mut k = mutate(&mut r, key);
            if r.below(4) == 0 {
                k = mutate(&mut r, &k);
            }
            same(&k, &mut cases, &mut found);
        }
    }

    let after = negs_core_sys::alloc_stats();
    println!(
        "help: {} paths and {} groups, {cases} lookups ({found} with a tooltip): 0 differences; core live allocations {} -> {} ({} made), bad frees {}",
        paths.len(),
        groups.len(),
        before.live_count,
        after.live_count,
        after.total_count - before.total_count,
        after.bad_frees
    );
    if after.tracked == 1 {
        assert_eq!(after.live_count, before.live_count, "the core leaked");
        assert_eq!(after.bad_frees, 0);
    }
}
