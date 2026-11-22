//! Photo facts, Odin core against the frozen Rust original
//! (core/diff/oracle/photo_facts.rs):
//! - camera names for every Unicode scalar value in five shapes (casing,
//!   final sigma, whitespace, the maker repeated, char boundaries), and
//!   seeded makers and models;
//! - shutter and every caption part for seeded and awkward numbers
//!   (ties, huge, negative, NaN, infinities, subnormals) and decimals;
//! - PhotoFacts::read on every file in ~/Downloads/free_raws (read only)
//!   and on seeded TIFFs with EXIF values (zero denominators, ISO 0),
//!   which the engine reads for both.
//! Where the original panics, the core's wrapper must too.
//! NEGS_RULES_SEEDS seeds (default 100, 100 cases each).

use negs_core_rules::photo_facts::PhotoFacts as Core;
use negs_media_oracle::photo_facts::PhotoFacts as Oracle;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

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
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
        from[self.below(from.len() as u64) as usize]
    }
}

/// The result, or that it panicked.
fn run<T>(f: impl FnOnce() -> T) -> Result<T, ()> {
    QUIET.set(true);
    let result = catch_unwind(AssertUnwindSafe(f)).map_err(|_| ());
    QUIET.set(false);
    result
}

thread_local! {
    /// While `run` expects a panic, which the log needn't show.
    static QUIET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[derive(Default)]
struct Tally {
    cameras: u64,
    panics: u64,
    numbers: u64,
    parts: u64,
    files: u64,
    read: u64,
}

fn same_camera(make: Option<&str>, model: Option<&str>, t: &mut Tally) {
    let expected = run(|| Oracle::camera(make, model));
    let got = run(|| Core::camera(make, model));
    assert_eq!(got, expected, "camera({make:?}, {model:?})");
    t.cameras += 1;
    t.panics += expected.is_err() as u64;
}

const DECIMALS: [char; 6] = ['.', ',', '٫', '·', 'x', '\u{10FFFF}'];

fn same_number(v: f64, t: &mut Tally) {
    for d in DECIMALS {
        assert_eq!(Core::shutter(v, d), Oracle::shutter(v, d), "shutter({v:e} = {:#x}, {d:?})", v.to_bits());
        let facts = (Some(v), Some(v), Some(v), Some(v));
        let core = Core { camera: None, iso: facts.0, exposure: facts.1, aperture: facts.2, focal_length: facts.3 };
        let oracle = Oracle { camera: None, iso: facts.0, exposure: facts.1, aperture: facts.2, focal_length: facts.3 };
        assert_eq!(core.parts(d), oracle.parts(d), "parts of {v:e} ({:#x}), {d:?}", v.to_bits());
    }
    t.numbers += 1;
}

fn same_parts(r: &mut Random, t: &mut Tally) {
    let maybe = |r: &mut Random| {
        let v = number(r);
        (r.below(3) != 0).then_some(v)
    };
    let camera = (r.below(3) != 0).then(|| ["Nikon D750", "", "Ünïcödé 📷", " x "][r.below(4) as usize].to_string());
    let (iso, exposure, aperture, focal) = (maybe(r), maybe(r), maybe(r), maybe(r));
    let core = Core { camera: camera.clone(), iso, exposure, aperture, focal_length: focal };
    let oracle = Oracle { camera, iso, exposure, aperture, focal_length: focal };
    let d = DECIMALS[r.below(DECIMALS.len() as u64) as usize];
    assert_eq!(core.parts(d), oracle.parts(d), "{oracle:?}");
    assert_eq!(core.items(d), oracle.items(d), "{oracle:?}");
    t.parts += 1;
}

/// A number a caption might get, or an awkward one.
fn number(r: &mut Random) -> f64 {
    match r.below(16) {
        0 => f64::from_bits(r.next()),
        1 => r.unit() * 0.3,
        2 => 1.0 / (1 + r.below(8000)) as f64,
        3 => 1.0 / ((1 + r.below(100)) as f64 + (r.unit() - 0.5) * 0.2),
        4 => (r.below(100_000) as f64) / 10.0 + 0.05,
        5 => (r.below(1000) as f64) * 0.25,
        // Near 2^49..2^53, where tenths tie.
        6 => (1u64 << (49 + r.below(4))) as f64 + (r.below(8) as f64) * 0.125,
        7 => -(r.unit() * 100.0),
        8 => [0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, f64::MIN_POSITIVE, 5e-324, f64::MAX, -f64::MAX, 0.3, 1.0, 0.95, 0.05, 0.25, 9.2e18, -9.3e18][r.below(16) as usize],
        9 => r.unit() * 1e6,
        10 => (r.below(64) as f64) * 0.1,
        _ => r.unit() * 30.0,
    }
}

const MAKES: [&str; 22] = [
    "NIKON CORPORATION", "SONY", "Canon", "FUJIFILM", "OLYMPUS IMAGING CORP.", "Panasonic", "LEICA CAMERA AG", "RICOH IMAGING COMPANY, LTD.",
    "Hasselblad", "", " ", "\t\n", "ΣΑΣ", "İSTANBUL", "ǅungla", "Ⅻ CAM", "AB", "ABC", "\u{212A}ODAK", "ẞTRASSE", "Ωmega", "\u{3000}GO PRO",
];
const MODELS: [&str; 18] = [
    "NIKON Z f", "ILCE-7M3", "Canon EOS R5", "X100V", "E-M1MarkII", "DC-S1M2", "LEICA M11", "", "  ", "nikon d750", "Sony", "ΣΑΣ 1", "i̇stanbul 2",
    "kodak", "\u{212A}x", "ss", "\u{00A0}X-T5\u{2003}", "GR III",
];

fn shout(r: &mut Random, s: &str) -> String {
    match r.below(4) {
        0 => s.to_uppercase(),
        1 => s.to_lowercase(),
        _ => s.to_string(),
    }
}

fn same_read(path: &Path, t: &mut Tally) {
    let expected = Oracle::read(path);
    let got = Core::read(path);
    let as_core = expected.clone().map(|e| Core { camera: e.camera, iso: e.iso, exposure: e.exposure, aperture: e.aperture, focal_length: e.focal_length });
    assert_eq!(got, as_core, "{}", path.display());
    t.files += 1;
    t.read += expected.is_some() as u64;
}

/// A little-endian TIFF the engine reads for EXIF: IFD0 with Make and
/// Model and an Exif IFD with exposure, f-number, ISO and focal length.
fn exif_tiff(r: &mut Random) -> Vec<u8> {
    let mut strings: Vec<(u16, Vec<u8>)> = vec![];
    if r.below(5) != 0 {
        strings.push((0x010f, format!("{}\0", r.pick(&MAKES)).into_bytes()));
    }
    if r.below(5) != 0 {
        strings.push((0x0110, format!("{}\0", r.pick(&MODELS)).into_bytes()));
    }
    let rational = |r: &mut Random| -> (u32, u32) {
        let n = [0u32, 1, 10, 28, 35, 1000, u32::MAX][r.below(7) as usize];
        let d = [0u32, 1, 10, 160, 1250, 4000, u32::MAX][r.below(7) as usize];
        (n, d)
    };
    let mut exif: Vec<(u16, u16, Vec<u8>)> = vec![];
    for tag in [0x829au16, 0x829d, 0x920a] {
        if r.below(4) != 0 {
            let (n, d) = rational(r);
            let mut v = n.to_le_bytes().to_vec();
            v.extend(d.to_le_bytes());
            exif.push((tag, 5, v));
        }
    }
    if r.below(3) != 0 {
        exif.push((0x8827, 3, ([0u16, 100, 400, 6400, u16::MAX][r.below(5) as usize]).to_le_bytes().to_vec()));
    }
    if r.below(3) != 0 {
        exif.push((0x8833, 4, ([0u32, 200, 800, 102400][r.below(4) as usize]).to_le_bytes().to_vec()));
    }
    exif.sort_by_key(|e| e.0);
    // Layout: header, IFD0 (strings, ExifIFD pointer), Exif IFD, data.
    let ifd0_entries = strings.len() + 1;
    let ifd0 = 8usize;
    let exif_at = ifd0 + 2 + ifd0_entries * 12 + 4;
    let mut data_at = exif_at + 2 + exif.len() * 12 + 4;
    let mut data: Vec<u8> = vec![];
    let mut f = b"II*\0".to_vec();
    f.extend((ifd0 as u32).to_le_bytes());
    f.extend((ifd0_entries as u16).to_le_bytes());
    let entry = |f: &mut Vec<u8>, data: &mut Vec<u8>, data_at: &mut usize, tag: u16, kind: u16, count: u32, value: &[u8]| {
        f.extend(tag.to_le_bytes());
        f.extend(kind.to_le_bytes());
        f.extend(count.to_le_bytes());
        if value.len() <= 4 {
            let mut v = value.to_vec();
            v.resize(4, 0);
            f.extend(v);
        } else {
            f.extend((*data_at as u32).to_le_bytes());
            data.extend(value);
            *data_at += value.len();
        }
    };
    for (tag, s) in &strings {
        entry(&mut f, &mut data, &mut data_at, *tag, 2, s.len() as u32, s);
    }
    entry(&mut f, &mut data, &mut data_at, 0x8769, 4, 1, &(exif_at as u32).to_le_bytes());
    f.extend(0u32.to_le_bytes());
    f.extend((exif.len() as u16).to_le_bytes());
    for (tag, kind, v) in &exif {
        let count = if *kind == 5 { 1 } else { 1 };
        entry(&mut f, &mut data, &mut data_at, *tag, *kind, count, v);
    }
    f.extend(0u32.to_le_bytes());
    f.extend(data);
    f
}

#[test]
fn the_core_makes_the_same_captions() {
    let before = negs_core_sys::alloc_stats();
    // Both sides panic on purpose here and there; keep the log quiet.
    let hook = std::sync::Arc::new(std::panic::take_hook());
    let quiet = hook.clone();
    std::panic::set_hook(Box::new(move |info| {
        if !QUIET.get() {
            quiet(info)
        }
    }));
    let mut t = Tally::default();

    // Every scalar value, in five shapes.
    for c in (0..=0x10ffffu32).filter_map(char::from_u32) {
        let lower: String = c.to_lowercase().collect();
        same_camera(Some(&format!("{c}{c}{c}")), Some(&format!("{c}{c}{c}X {c}")), &mut t);
        same_camera(Some(&format!("{c}Σ")), Some(&format!("{lower}ς rest")), &mut t);
        same_camera(Some(&format!("A{c}Σ")), Some(&format!("a{lower}ς rest")), &mut t);
        same_camera(Some(&format!("AΣ{c}")), Some(&format!("aς{lower} rest")), &mut t);
        same_camera(Some(&format!("XY{c}Σ{c}")), Some(&format!(" {c}m{c} ")), &mut t);
    }
    let exhaustive = t.cameras;

    // Seeded makers, models, numbers and parts.
    let seeds: u64 = std::env::var("NEGS_RULES_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(100);
    for seed in 0..seeds {
        let mut r = Random(seed ^ 0xfac7);
        for _ in 0..100 {
            let make = (r.below(6) != 0).then(|| {
                let base = r.pick(&MAKES);
                let mut m = shout(&mut r, base);
                if r.below(4) == 0 {
                    m = format!("{}{m}{}", r.pick(&["", " ", "\t", "\u{2028}"]), r.pick(&["", " ", " x", "\u{85}"]));
                }
                m
            });
            let model = (r.below(6) != 0).then(|| {
                let base = if r.below(3) == 0 { format!("{} {}", make.as_deref().unwrap_or(""), r.pick(&MODELS)) } else { r.pick(&MODELS).to_string() };
                shout(&mut r, &base)
            });
            same_camera(make.as_deref(), model.as_deref(), &mut t);
            same_number(number(&mut r), &mut t);
            same_parts(&mut r, &mut t);
        }
    }

    // PhotoFacts::read: the test RAWs (and the files beside them), and
    // seeded TIFFs with EXIF values.
    let folder = std::env::var_os("NEGS_RAW_FOLDER").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join("Downloads/free_raws")));
    if let Some(folder) = folder.filter(|f| f.is_dir()) {
        let mut files: Vec<PathBuf> = vec![];
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(std::fs::read_dir(&path).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_file()).take(10));
            } else {
                files.push(path);
            }
        }
        files.sort();
        for path in &files {
            same_read(path, &mut t);
        }
    } else {
        eprintln!("photo_facts: no ~/Downloads/free_raws (NEGS_RAW_FOLDER): synthetic files only");
    }
    let scratch = std::env::temp_dir().join(format!("negs-core-facts-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let mut r = Random(0xe71f);
    for i in 0..300 {
        let path = scratch.join(format!("{i}.tif"));
        std::fs::write(&path, exif_tiff(&mut r)).unwrap();
        same_read(&path, &mut t);
    }
    let _ = std::fs::remove_dir_all(&scratch);
    let _ = std::panic::take_hook();

    let after = negs_core_sys::alloc_stats();
    println!(
        "photo_facts: {} camera names ({exhaustive} over every scalar value, {} where both panic), {} numbers × {} decimals, {} part sets, {} files read ({} with facts): 0 differences; core live allocations {} -> {} ({} made), bad frees {}",
        t.cameras,
        t.panics,
        t.numbers,
        DECIMALS.len(),
        t.parts,
        t.files,
        t.read,
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
