//! The Mac ↔ Windows sidecar round trip: one folder edited by the core on
//! one machine, read and edited on the other, and back.
//!
//!     roundtrip make  <dir>                  photographs + the Mac's fixtures
//!     roundtrip step  <dir> <seed> <steps>   seeded edits by the core
//!     roundtrip check <dir>                  read by core and Rust on copies
//!
//! `check` opens a copy of the folder with the core and another with the
//! Rust oracle and asserts that both show the same, that opening changed no
//! byte, and that every sidecar reads and writes back byte for byte in both.
//! It prints two digests: the files' bytes and what the library shows, with
//! paths made relative, so two machines' checks of the same folder can be
//! compared line for line. `step` is deterministic (seeded ids, no clocks
//! in sidecars), so the same step on two machines must write the same bytes.

use negs_library::{Config, EditGroup, Flag, FolderBin, Item, Json};
use negs_library_rust::sha256::sha256;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn data() -> PathBuf {
    std::env::var_os("NEGS_DATA").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"))
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/sidecar-fixtures")
}

fn config(root: &Path, seed: u64) -> Config {
    Config {
        data: data(),
        app: "roundtrip".into(),
        bin: Box::new(FolderBin::new(root.join("bin"))),
        pairing: true,
        sidecars: true,
        id_seed: Some(seed),
    }
}

/// `open <folder> [rust]`: how long a fresh process takes to open a real
/// folder (as the app does with the photograph it was given), and, for the
/// core, its first call (the runtime's startup). The bin is a scratch folder.
fn open_timed(folder: &Path, rust: bool) {
    let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
    let scratch = std::env::temp_dir().join(format!("negs-roundtrip-open-{}", std::process::id()));
    let config = |seed| Config { bin: Box::new(FolderBin::new(scratch.join("bin"))), id_seed: None, ..config(&scratch, seed) };
    let t = std::time::Instant::now();
    if !rust {
        // One that runs in the core's context, so the runtime starts.
        unsafe { negs_core_sys::theme::negs_theme_parse(c"darkroom".as_ptr(), 8) };
    }
    let first = ms(t);
    // The Windows app's calls on opening a photograph, one by one.
    macro_rules! steps {
        ($lib:expr) => {{
            let t = std::time::Instant::now();
            let mut lib = $lib;
            let mut times = vec![("new", ms(t))];
            let t = std::time::Instant::now();
            let defaults = lib.defaults().clone();
            lib.set_defaults(defaults);
            times.push(("set_defaults", ms(t)));
            let items = lib.items();
            let t = std::time::Instant::now();
            if let Some(item) = items.first() {
                lib.open(item);
            }
            times.push(("open", ms(t)));
            let t = std::time::Instant::now();
            let state = lib.state().cloned();
            times.push(("state", ms(t)));
            if let Some(state) = state {
                let t = std::time::Instant::now();
                lib.edit(state);
                times.push(("edit", ms(t)));
            }
            let t = std::time::Instant::now();
            for _ in 0..10 {
                std::hint::black_box(lib.unedited_state());
            }
            times.push(("unedited_state x10", ms(t)));
            let t = std::time::Instant::now();
            drop(lib);
            times.push(("drop", ms(t)));
            (items.len(), times)
        }};
    }
    let (items, times) = if rust {
        steps!(negs_library_rust::Library::new(folder.to_path_buf(), config(0)).unwrap())
    } else {
        steps!(negs_library::Library::new(folder.to_path_buf(), config(0)).unwrap())
    };
    let _ = std::fs::remove_dir_all(&scratch);
    let total: f64 = times.iter().map(|t| t.1).sum();
    let steps: Vec<String> = times.iter().map(|(n, t)| format!("{n} {t:.2}")).collect();
    println!("{} first call {first:.2} ms, {items} items, {total:.2} ms: {}", if rust { "rust" } else { "core" }, steps.join(", "));
}

/// SplitMix64.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())].clone()
    }
    fn subset<T: Clone>(&mut self, items: &[T]) -> Vec<T> {
        let some: Vec<T> = items.iter().filter(|_| self.next() % 2 == 0).cloned().collect();
        if some.is_empty() { vec![self.pick(items)] } else { some }
    }
}

fn make(dir: &Path) {
    let photos = dir.join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let names = ["a.png", "b.png", "c.png", "d.png", "e.png", "f.CR3", "f.JPG", "Ä 10.png", "ä 9.png", "Ωmega.CR3", "ωmega.JPG", "東京 2.png", "g.NEF"];
    for (index, name) in names.iter().enumerate() {
        std::fs::write(photos.join(name), [b'p', b'h', b'o', b't', index as u8]).unwrap();
    }
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let entry = entry.unwrap();
        let fixture = entry.file_name().to_string_lossy().into_owned();
        if let Some(stem) = fixture.strip_suffix(".negs") {
            // Each fixture as the sidecar of a photograph of its own.
            let photo = format!("fixture {stem}.png");
            std::fs::write(photos.join(&photo), b"phot").unwrap();
            std::fs::copy(entry.path(), photos.join(format!("{photo}.negs"))).unwrap();
        }
    }
    println!("made {}", dir.display());
}

const FILMS: [&str; 3] = ["kodak_portra_400", "kodak_ektar_100", "kodak_gold_200"];
const SLIDERS: [&str; 3] = ["prep.contrast", "prep.highlights", "prep.shadows"];

fn step(dir: &Path, seed: u64, steps: usize) {
    let mut lib = negs_library::Library::new(dir.join("photos"), config(dir, seed)).unwrap();
    if lib.items().is_empty() {
        panic!("no photographs in {}", dir.join("photos").display());
    }
    let mut random = Random(seed);
    let mut log = Vec::new();
    for _ in 0..steps {
        let items: Vec<Item> = lib.items();
        let what = match random.below(10) {
            0 | 1 => {
                let targets = random.subset(&items);
                let stars = random.below(6) as i64;
                lib.set_rating(&targets, stars);
                format!("rate {} {stars}", targets.len())
            }
            2 => {
                let targets = random.subset(&items);
                let flag = random.pick(&[None, Some(Flag::Pick), Some(Flag::Reject)]);
                lib.set_flag(&targets, flag);
                format!("flag {} {flag:?}", targets.len())
            }
            3..=5 => {
                let item = random.pick(&items);
                lib.open(&item);
                let Some(mut state) = lib.state().cloned() else { continue };
                if random.below(2) == 0 {
                    state.film = random.pick(&FILMS).into();
                } else {
                    let value = (random.below(9) as f64 - 4.0) * 10.0 + random.below(3) as f64 * 0.125;
                    state.params.set_at(random.pick(&SLIDERS), Json::Number(value));
                }
                lib.edit(state);
                lib.save().unwrap();
                lib.close().unwrap();
                format!("edit {}", lib.name(&item))
            }
            6 => {
                let item = random.pick(&items);
                lib.copy_edits(&item, EditGroup::standard());
                let targets = random.subset(&items);
                lib.paste_edits(&targets);
                format!("copy {} paste {}", lib.name(&item), targets.len())
            }
            7 => {
                let targets = vec![random.pick(&items)];
                lib.duplicate(&targets);
                format!("duplicate {}", lib.name(&targets[0]))
            }
            8 => {
                let targets = vec![random.pick(&items)];
                lib.reset_edits(&targets);
                format!("reset {}", lib.name(&targets[0]))
            }
            _ => {
                lib.undo();
                "undo".to_string()
            }
        };
        log.push(what);
    }
    lib.close().unwrap();
    println!("step seed {seed}: {}", log.join("; "));
}

fn folder_bytes(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for sub in ["photos", "bin"] {
        if let Ok(entries) = std::fs::read_dir(dir.join(sub)) {
            for entry in entries {
                let entry = entry.unwrap();
                if entry.path().is_file() {
                    files.insert(format!("{sub}/{}", entry.file_name().to_string_lossy()), std::fs::read(entry.path()).unwrap());
                }
            }
        }
    }
    files
}

fn copy_folder(from: &Path, to: &Path) {
    for (name, bytes) in folder_bytes(from) {
        let path = to.join(&name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    std::fs::create_dir_all(to.join("bin")).unwrap();
}

macro_rules! view {
    ($lib:expr, $root:expr) => {{
        let lib = &$lib;
        let mut lines = Vec::new();
        let items = lib.items();
        lines.push(format!("items {:?}", items.iter().map(|i| lib.name(i)).collect::<Vec<_>>()));
        lines.push(format!("shown {:?}", lib.shown(None).iter().map(|i| lib.name(i)).collect::<Vec<_>>()));
        lines.push(format!("rejected {:?}", lib.rejected().iter().map(|i| lib.name(i)).collect::<Vec<_>>()));
        for item in &items {
            lines.push(format!("{} summary {:?} look {:?}", lib.name(item), lib.summary(item), lib.look(item)));
            lines.push(format!("{} thumbnail {:?} export {:?}", lib.name(item), lib.thumbnail_state(item), lib.export_edits(item)));
        }
        lines.push(format!("defaults {:?}", lib.defaults()));
        relative(lines, $root)
    }};
}

/// Paths made relative and separators '/', so two machines compare.
fn relative(lines: Vec<String>, root: &Path) -> Vec<String> {
    let plain = root.display().to_string();
    let debug = format!("{:?}", root.display().to_string());
    let debug = debug.trim_matches('"').to_string();
    lines
        .into_iter()
        .map(|l| l.replace(&debug, "<root>").replace(&plain, "<root>").replace("\\\\", "/").replace('\\', "/"))
        .collect()
}

fn digest(parts: impl IntoIterator<Item = Vec<u8>>) -> String {
    let mut all = Vec::new();
    for p in parts {
        all.extend_from_slice(&(p.len() as u64).to_le_bytes());
        all.extend(p);
    }
    sha256(&all).iter().map(|b| format!("{b:02x}")).collect()
}

fn check(dir: &Path) {
    let before = folder_bytes(dir);
    let scratch = std::env::temp_dir().join(format!("negs-roundtrip-{}", std::process::id()));
    let (core_root, rust_root) = (scratch.join("core"), scratch.join("rust"));
    copy_folder(dir, &core_root);
    copy_folder(dir, &rust_root);
    let core_view = {
        let lib = negs_library::Library::new(core_root.join("photos"), config(&core_root, 1)).unwrap();
        view!(lib, &core_root)
    };
    let rust_view = {
        let lib = negs_library_rust::Library::new(rust_root.join("photos"), config(&rust_root, 1)).unwrap();
        view!(lib, &rust_root)
    };
    let mut failures = Vec::new();
    for (i, (c, r)) in core_view.iter().zip(&rust_view).enumerate() {
        if c != r {
            failures.push(format!("view line {i}:\n  core {c}\n  rust {r}"));
        }
    }
    if core_view.len() != rust_view.len() {
        failures.push(format!("view lengths {} and {}", core_view.len(), rust_view.len()));
    }
    for (side, root) in [("core", &core_root), ("rust", &rust_root)] {
        if folder_bytes(root) != before {
            failures.push(format!("opening with the {side} changed the folder"));
        }
    }
    // Fixtures as committed use ": " where the Mac writes " : ", so a sidecar
    // nobody has written since `make` rewrites to the Mac's spacing; every
    // other one must already be what the Mac writes. Core and Rust always agree.
    let originals: Vec<Vec<u8>> =
        std::fs::read_dir(fixtures()).unwrap().map(|e| std::fs::read(e.unwrap().path()).unwrap()).collect();
    let (mut sidecars, mut untouched) = (0, 0);
    for (name, bytes) in &before {
        if !name.ends_with(".negs") {
            continue;
        }
        sidecars += 1;
        let fixture = originals.contains(bytes);
        untouched += fixture as usize;
        let oracle = Json::parse(bytes).map(|j| j.to_sidecar_bytes()).map_err(|e| format!("{e:?}"));
        let core = negs_library::core_json::sidecar_rewrite(bytes);
        match (oracle, core) {
            (Ok(o), Ok(c)) if o == c && (fixture || o == *bytes) => {}
            (o, c) => failures.push(format!(
                "{name}: rust {:?} core {:?} (fixture {fixture})",
                o.map(|o| if o == *bytes { "same bytes" } else { "other bytes" }),
                c.map(|c| if c == *bytes { "same bytes" } else { "other bytes" })
            )),
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    let files = digest(before.iter().map(|(n, b)| [n.as_bytes(), b.as_slice()].concat()));
    let view = digest(core_view.iter().map(|l| l.as_bytes().to_vec()));
    println!("files {} sidecars {sidecars} ({untouched} untouched fixtures) digest {files}", before.len());
    println!("view lines {} digest {view}", core_view.len());
    if let Some(path) = std::env::var_os("ROUNDTRIP_VIEW") {
        std::fs::write(path, core_view.join("\n")).unwrap();
    }
    if failures.is_empty() {
        println!("CHECK OK");
    } else {
        for f in &failures {
            println!("FAIL {f}");
        }
        std::process::exit(1);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(args.get(2).expect("a folder"));
    match args.get(1).map(String::as_str) {
        Some("make") => make(&dir),
        Some("step") => step(&dir, args[3].parse().unwrap(), args[4].parse().unwrap()),
        Some("check") => check(&dir),
        Some("open") => open_timed(&dir, args.get(3).map(String::as_str) == Some("rust")),
        _ => panic!("roundtrip make|step|check <dir> ..."),
    }
    // Under `--features track`, nothing the core allocated is left.
    let stats = negs_core_sys::alloc_stats();
    if stats.tracked != 0 && (stats.live_count != 0 || stats.bad_frees != 0) {
        eprintln!("the core leaked: {stats:?}");
        std::process::exit(101);
    }
}
