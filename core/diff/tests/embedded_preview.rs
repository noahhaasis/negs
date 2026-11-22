//! Embedded previews, Odin core against the frozen Rust original
//! (core/diff/oracle/embedded_preview.rs): every file in
//! ~/Downloads/free_raws (or NEGS_RAW_FOLDER; read only) by path and as
//! bytes, with every preview's bytes; each one truncated and corrupted; then
//! seeded synthetic TIFF and RAF headers (cycles, absurd counts, offsets
//! past the end, SubIFD lists, Exif IFDs, JPEG markers) as bytes and as
//! files; and the choice between previews. Every offset, length, size,
//! orientation and ratio must match. NEGS_RULES_SEEDS seeds (default 100,
//! 100 headers each).

#[allow(dead_code, unused_imports)]
#[path = "../oracle/embedded_preview.rs"]
mod oracle;

use negs_core_rules::embedded_preview as core;
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
    fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }
}

type Found = Option<(Vec<(u64, u64, u32, u32)>, Option<u16>)>;

fn of_oracle(p: Option<oracle::Previews>) -> Found {
    p.map(|p| (p.jpegs.iter().map(|j| (j.offset, j.length, j.width, j.height)).collect(), p.orientation))
}

fn of_core(p: Option<core::Previews>) -> Found {
    p.map(|p| (p.jpegs.iter().map(|j| (j.offset, j.length, j.width, j.height)).collect(), p.orientation))
}

#[derive(Default)]
struct Tally {
    files: u64,
    with_previews: u64,
    jpeg_bytes: u64,
    headers: u64,
    found: u64,
    choices: u64,
}

fn same_bytes(data: &[u8], t: &mut Tally, what: &str) {
    let expected = of_oracle(oracle::find_in(&mut &data[..]));
    let got = of_core(core::find_in(&mut &data[..]));
    assert_eq!(got, expected, "{what}");
    t.headers += 1;
    if expected.as_ref().is_some_and(|(j, _)| !j.is_empty()) {
        t.found += 1;
    }
}

fn same_file(path: &Path, t: &mut Tally) -> Found {
    let expected = of_oracle(oracle::find(path));
    let got = of_core(core::find(path));
    assert_eq!(got, expected, "{}", path.display());
    t.files += 1;
    if let Some((jpegs, _)) = &expected {
        t.with_previews += 1;
        for &(offset, length, width, height) in jpegs {
            let a = oracle::bytes(path, &oracle::Jpeg { offset, length, width, height });
            let b = core::bytes(path, &core::Jpeg { offset, length, width, height });
            assert!(a == b, "{}: the bytes at {offset}", path.display());
            t.jpeg_bytes += length;
        }
    }
    expected
}

fn same_choice(jpegs: &[(u64, u64, u32, u32)], orientation: Option<u16>, pixels: &[u32], t: &mut Tally) {
    let o = oracle::Previews { jpegs: jpegs.iter().map(|&(offset, length, width, height)| oracle::Jpeg { offset, length, width, height }).collect(), orientation };
    let c = core::Previews { jpegs: jpegs.iter().map(|&(offset, length, width, height)| core::Jpeg { offset, length, width, height }).collect(), orientation };
    let tuple = |j: Option<(u64, u64, u32, u32)>| j;
    assert_eq!(tuple(c.largest().map(|j| (j.offset, j.length, j.width, j.height))), o.largest().map(|j| (j.offset, j.length, j.width, j.height)), "largest of {jpegs:?}");
    for &px in pixels {
        assert_eq!(c.for_size(px).map(|j| (j.offset, j.length, j.width, j.height)), o.for_size(px).map(|j| (j.offset, j.length, j.width, j.height)), "for_size({px}) of {jpegs:?}");
    }
    assert_eq!(c.ratio().map(f64::to_bits), o.ratio().map(f64::to_bits), "ratio of {jpegs:?} {orientation:?}");
    t.choices += 1;
}

/// The bytes of a little or big endian u16/u32.
fn put16(out: &mut Vec<u8>, little: bool, v: u16) {
    out.extend(if little { v.to_le_bytes() } else { v.to_be_bytes() });
}
fn put32(out: &mut Vec<u8>, little: bool, v: u32) {
    out.extend(if little { v.to_le_bytes() } else { v.to_be_bytes() });
}

/// A JPEG-ish blob: SOI, some markers (fill bytes, APPn, maybe a SOF of
/// any kind, maybe garbage), padded.
fn jpeg_blob(r: &mut Random) -> Vec<u8> {
    if r.chance(2) {
        // A plain baseline or progressive one.
        let (w, h) = (1 + r.below(8000) as u16, 1 + r.below(6000) as u16);
        let mut j = vec![0xff, 0xd8, 0xff, 0xe1, 0x00, 0x04, 0, 0, 0xff, if r.chance(2) { 0xc0 } else { 0xc2 }, 0x00, 0x11, 8];
        j.extend(h.to_be_bytes());
        j.extend(w.to_be_bytes());
        j.resize(64 + r.below(300) as usize, 0);
        return j;
    }
    let mut j = vec![0xff, if r.chance(20) { 0xd9 } else { 0xd8 }];
    for _ in 0..r.below(5) {
        match r.below(6) {
            0 => j.push(0xff),
            1 => j.extend([0xff, 0xe0 + r.below(16) as u8, 0, 2 + r.below(30) as u8]),
            2 => j.extend([0xff, 0xdb, 0, 4, 0, 0]),
            3 => j.extend([0x12, 0x34]),
            _ => {
                let len = 2 + r.below(10) as usize;
                j.extend([0xff, 0xe1, 0, len as u8]);
                j.extend(std::iter::repeat_n(0u8, len.saturating_sub(2)));
            }
        }
    }
    if !r.chance(5) {
        let sof = [0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc9, 0xcd, 0xda, 0xd8, 0xc4][r.below(10) as usize];
        let (h, w) = (r.below(9000) as u16 * if r.chance(15) { 0 } else { 1 }, r.below(9000) as u16);
        j.extend([0xff, sof, 0, 17, 8]);
        j.extend(h.to_be_bytes());
        j.extend(w.to_be_bytes());
    }
    let pad = [0usize, 10, 64, 200, 1000][r.below(5) as usize];
    j.resize(j.len().max(pad), 0);
    j
}

/// A synthetic TIFF: IFDs laid out at random, chained and nested, with the
/// tags the scan reads given random kinds, counts and offsets (often to
/// another IFD, a JPEG, or past the end), then JPEG blobs.
fn tiff(r: &mut Random) -> Vec<u8> {
    let little = r.chance(2);
    let magic = [42u16, 0x4f52, 0x5352, 0x55, 43, 0][r.below(6) as usize];
    let ifds = 1 + r.below(6) as usize;
    let mut sizes = vec![];
    for _ in 0..ifds {
        sizes.push(if r.chance(30) { 1001 + r.below(100) as usize } else { r.below(12) as usize });
    }
    // Where everything goes.
    let mut at = 8usize;
    let mut ifd_at = vec![];
    for &n in &sizes {
        ifd_at.push(at as u32);
        at += 2 + n.min(20) * 12 + 4;
    }
    let lists_at = at as u32;
    let list_len = 1 + r.below(40) as usize;
    at += list_len * 4;
    let blobs: Vec<Vec<u8>> = (0..1 + r.below(5)).map(|_| jpeg_blob(r)).collect();
    let mut blob_at = vec![];
    for b in &blobs {
        blob_at.push(at as u32);
        at += b.len();
    }
    let total = at as u32;
    let target = |r: &mut Random| -> u32 {
        match r.below(8) {
            0 => ifd_at[r.below(ifds as u64) as usize],
            1 => lists_at,
            2 => total + r.below(1000) as u32,
            3 => r.next() as u32,
            4 => 0,
            _ => blob_at[r.below(blob_at.len() as u64) as usize],
        }
    };
    let mut f = vec![];
    f.extend(if little { *b"II" } else { *b"MM" });
    put16(&mut f, little, magic);
    put32(&mut f, little, if r.chance(20) { target(r) } else { ifd_at[0] });
    for (i, &n) in sizes.iter().enumerate() {
        put16(&mut f, little, n as u16);
        let mut last_blob = 0usize;
        for _ in 0..n.min(20) {
            let tag = [0x0103u16, 0x0111, 0x0117, 0x0201, 0x0202, 0x002e, 0x0112, 0x014a, 0x8769, 0x0100, 0x0111, 0x0117][r.below(12) as usize];
            let kind = if r.chance(2) { 4 } else { [3u16, 4, 13, 7, 1, 3, 4][r.below(7) as usize] };
            let count = if r.chance(2) { 1 } else { [1u32, 1, 1, 2, 0, 5, 40, 300][r.below(8) as usize] };
            put16(&mut f, little, tag);
            put16(&mut f, little, kind);
            put32(&mut f, little, if tag == 0x002e && r.chance(2) { blobs[0].len() as u32 } else { count });
            let value = match tag {
                0x0103 => [6u32, 7, 1, 6][r.below(4) as usize],
                0x0112 => r.below(10) as u32,
                // A length mostly of the blob the last offset named.
                0x0117 | 0x0202 => blobs[last_blob].len() as u32 + if r.chance(5) { r.below(100) as u32 } else { 0 },
                0x0111 | 0x0201 if r.chance(2) => {
                    last_blob = r.below(blobs.len() as u64) as usize;
                    blob_at[last_blob]
                }
                _ => target(r),
            };
            if kind == 3 {
                // A SHORT's first value sits in the first two bytes.
                put16(&mut f, little, value as u16);
                put16(&mut f, little, r.next() as u16);
            } else {
                put32(&mut f, little, value);
            }
        }
        let next = if i + 1 < ifds && r.chance(2) { ifd_at[i + 1] } else if r.chance(6) { target(r) } else { 0 };
        put32(&mut f, little, next);
        assert_eq!(f.len(), (ifd_at[i] as usize) + 2 + n.min(20) * 12 + 4);
    }
    for _ in 0..list_len {
        put32(&mut f, little, target(r));
    }
    for b in &blobs {
        f.extend(b);
    }
    if r.chance(4) {
        let cut = r.below(f.len() as u64 + 1) as usize;
        f.truncate(cut);
    }
    f
}

fn raf(r: &mut Random) -> Vec<u8> {
    let mut f = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
    f.resize(84, 0);
    let blob = jpeg_blob(r);
    let at = if r.chance(5) { r.next() as u32 } else { 92 + r.below(10) as u32 };
    let len = if r.chance(5) { r.next() as u32 } else { blob.len() as u32 };
    f.extend(at.to_be_bytes());
    f.extend(len.to_be_bytes());
    f.resize(at.min(200) as usize, 0);
    f.extend(blob);
    if r.chance(4) {
        let cut = r.below(f.len() as u64 + 1) as usize;
        f.truncate(cut);
    }
    f
}

/// Random byte changes to a real header.
fn corrupt(r: &mut Random, data: &[u8]) -> Vec<u8> {
    let mut d = data.to_vec();
    for _ in 0..1 + r.below(8) {
        if d.is_empty() {
            break;
        }
        let i = r.below(d.len().min(4096) as u64) as usize;
        match r.below(3) {
            0 => d[i] = r.next() as u8,
            1 => d[i] ^= 1 << r.below(8),
            _ => {
                let v = r.next() as u32;
                for k in 0..4 {
                    if i + k < d.len() {
                        d[i + k] = (v >> (8 * k)) as u8;
                    }
                }
            }
        }
    }
    d
}

fn raw_folder() -> Option<PathBuf> {
    let folder = std::env::var_os("NEGS_RAW_FOLDER").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join("Downloads/free_raws")))?;
    folder.is_dir().then_some(folder)
}

#[test]
fn the_core_finds_what_rust_finds() {
    let before = negs_core_sys::alloc_stats();
    let mut t = Tally::default();
    let mut r = Random(0x3c_e3b);

    // The test RAWs, by path and as bytes, whole, cut and corrupted.
    let mut headers: Vec<Vec<u8>> = vec![];
    match raw_folder() {
        Some(folder) => {
            let mut files: Vec<PathBuf> = std::fs::read_dir(&folder).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
            files.sort();
            for path in &files {
                let found = same_file(path, &mut t);
                let data = std::fs::read(path).unwrap();
                same_bytes(&data, &mut t, &path.display().to_string());
                // Cut inside the header, inside each preview, and at its end.
                let mut cuts: Vec<usize> = (0..8).map(|_| r.below(data.len().min(1 << 16) as u64) as usize).collect();
                if let Some((jpegs, orientation)) = &found {
                    for &(offset, length, ..) in jpegs {
                        cuts.extend([offset as usize + 1, (offset + length / 2) as usize, (offset + length) as usize - 1, (offset + length) as usize]);
                    }
                    same_choice(jpegs, *orientation, &[0, 160, 432, 768, 1620, 4000, 10000, u32::MAX], &mut t);
                }
                for cut in cuts {
                    same_bytes(&data[..cut.min(data.len())], &mut t, &format!("{} cut at {cut}", path.display()));
                }
                let head = &data[..data.len().min(1 << 16)];
                for _ in 0..10 {
                    let bad = corrupt(&mut r, head);
                    same_bytes(&bad, &mut t, &format!("{} corrupted", path.display()));
                }
                headers.push(head.to_vec());
            }
            assert!(t.files >= 1);
        }
        None => eprintln!("embedded_preview: no ~/Downloads/free_raws (NEGS_RAW_FOLDER): synthetic headers only"),
    }

    // Seeded synthetic headers, as bytes, and some as files.
    let seeds: u64 = std::env::var("NEGS_RULES_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(100);
    let scratch = std::env::temp_dir().join(format!("negs-core-embedded-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    for seed in 0..seeds {
        let mut r = Random(seed.wrapping_mul(0x9E37_79B9) ^ 0x3c);
        for case in 0..100 {
            let data = match r.below(10) {
                0 => raf(&mut r),
                1 if !headers.is_empty() => {
                    let which = r.below(headers.len() as u64) as usize;
                    corrupt(&mut r, &headers[which])
                }
                _ => tiff(&mut r),
            };
            same_bytes(&data, &mut t, &format!("seed {seed} case {case}: {data:?}"));
            if case % 25 == 0 {
                let path = scratch.join(format!("{seed}-{case}.nef"));
                std::fs::write(&path, &data).unwrap();
                same_file(&path, &mut t);
            }
            // The choice between previews: equal areas and lengths, zero
            // sizes, shapes near the tolerance.
            let n = r.below(7) as usize;
            let jpegs: Vec<(u64, u64, u32, u32)> = (0..n)
                .map(|i| {
                    let w = [0u32, 1, 160, 570, 1620, 3000, 6000, 4000][r.below(8) as usize];
                    let h = [0u32, 1, 120, 380, 1080, 2000, 4000, 2667, 2700][r.below(9) as usize];
                    (i as u64, [100u64, 1000, 1000, 5000][r.below(4) as usize], w, h)
                })
                .collect();
            let orientation = r.chance(2).then(|| r.below(10) as u16);
            same_choice(&jpegs, orientation, &[0, 1, 160, 433, 1620, 6000, u32::MAX, r.next() as u32], &mut t);
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    // A missing file, a folder, a path with a NUL.
    for path in [Path::new("/no/such/file.nef"), Path::new("/"), Path::new("a\0b")] {
        same_file(path, &mut t);
    }

    let after = negs_core_sys::alloc_stats();
    println!(
        "embedded_preview: {} files ({} with previews, {:.1} MB of preview bytes), {} headers ({} with previews), {} choices: 0 differences; core live allocations {} -> {} ({} made), bad frees {}",
        t.files,
        t.with_previews,
        t.jpeg_bytes as f64 / 1e6,
        t.headers,
        t.found,
        t.choices,
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
