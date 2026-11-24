//! presets.rs on the core (core/src/settings/presets, through
//! negs_core_rules::presets) against the frozen windows/src/presets.rs
//! (core/diff/oracle/presets.rs, with the state-oracle's stand-ins):
//! - the name rules (problem, key, unique, from_file_name) and `text` on
//!   random names, file names and bytes; the core's NFC is the stand-in's
//!   ICU NormalizeString, registered as the Windows wrapper registers
//!   Windows' own;
//! - a lockstep: seeded random sequences of preset operations (add, rename,
//!   delete, read, set_order, list, read_file) and of other programs'
//!   doings (files copied in, edited, removed or hidden, folders and order
//!   files put in, the folder removed), one scratch folder each, every
//!   result compared and both folders' bytes after every step;
//! - preset JSON (data/presets/spektrafilm-gui-default-28bf883.json and a
//!   random corpus) added, read back and written as serde's pretty writer
//!   does, byte for byte.
//! Seeded (NEGS_RULES_SEEDS, default 10,000; NEGS_RULES_SEED); each test
//! checks the core leaked nothing.

mod rules_common;
mod state_common;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use negs_core_rules::presets as core;
use negs_state_oracle::presets as oracle;
use rules_common::{Leaks, Random, Tally, cases, first_seed};
use state_common::*;

// MARK: The host's NFC

/// presets.rs's `nfc` on the oracles' NormalizeString (Windows' own on
/// Windows, ICU's elsewhere), as a negs_text_fn.
unsafe extern "C" fn nfc(_user: *mut std::ffi::c_void, text: *const u8, len: usize, out: *mut u8, cap: usize) -> usize {
    use negs_state_oracle::win::Win32::Globalization::{NormalizationC, NormalizeString};
    let text = core::text::take(text, len);
    let source: Vec<u16> = text.encode_utf16().collect();
    if source.is_empty() {
        return core::text::give("", out, cap);
    }
    let mut length = unsafe { NormalizeString(NormalizationC, &source, None) };
    for _ in 0..3 {
        if length <= 0 {
            break;
        }
        let mut buffer = vec![0u16; length as usize];
        let written = unsafe { NormalizeString(NormalizationC, &source, Some(&mut buffer)) };
        if written > 0 {
            return core::text::give(&String::from_utf16_lossy(&buffer[..written as usize]), out, cap);
        }
        length = -written;
    }
    core::text::give(&text, out, cap)
}

fn register() {
    // The wrapper's own registration first (once), then the stand-in's NFC.
    core::text::register();
    unsafe { negs_core_sys::settings::negs_settings_text_functions(Some(nfc), Some(core::text::lowercase), std::ptr::null_mut()) };
}

// MARK: Names

const NAMES: &[&str] = &[
    "Portra", "portra", "PORTRA", "Portra 2", "portra 3", "Gold", "gold", "caf\u{E9}", "cafe\u{301}", "CAF\u{C9}", "Ünïcødé", "ΣΑΣ", "σας",
    "İstanbul", "istanbul", "ǅ", "ß", "SS", "Ω", "Ω", "a/b", "a:b", "what?", "CON", "con.json", "nul.json", "COM1", "lpt9.x", "COM0", " lead",
    "trail.", "trail ", ".hidden", "", "   ", "\u{3000}", "tab\there", "v1.2", "Kodachrome – 1970s", "x.json", "x.JSON", "😀", "a\u{85}",
];

fn name(r: &mut Random) -> String {
    match r.below(6) {
        0 => string(r),
        1 => "x".repeat(118 + r.index(5)),
        2 => format!("{} {}", NAMES[r.index(NAMES.len())], 2 + r.below(3)),
        _ => NAMES[r.index(NAMES.len())].to_string(),
    }
}

fn file_name(r: &mut Random) -> String {
    let mut s = name(r);
    for _ in 0..r.index(4) {
        let at = (0..=s.len()).filter(|i| s.is_char_boundary(*i)).nth(r.index(s.chars().count() + 1)).unwrap_or(0);
        s.insert(at, ['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\n', '\r', '.', ' ', '\t'][r.index(14)]);
    }
    if r.chance(10) {
        s = s.repeat(1 + r.index(40));
    }
    s
}

fn bytes_of(r: &mut Random) -> Vec<u8> {
    match r.below(5) {
        0 => (0..r.index(12)).map(|_| r.below(256) as u8).collect(),
        1 => [b"\xEF\xBB\xBF".as_slice(), string(r).as_bytes()].concat(),
        2 => mutate(r, b"\xEF\xBB\xBF{}"),
        _ => json_text(r),
    }
}

#[test]
fn names_and_texts_alike() {
    let _serial = serial();
    register();
    let leaks = Leaks::start();
    let mut tally = Tally::default();
    let n = cases();
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed);
        let a = name(&mut r);
        tally.check("problem", seed, oracle::name::problem(&a), core::name::problem(&a));
        tally.check("key", seed, oracle::name::key(&a), core::name::key(&a));
        let taken: HashSet<String> = (0..r.index(6)).map(|_| oracle::name::key(&name(&mut r))).collect();
        let mut taken = taken;
        if r.bool() {
            taken.insert(oracle::name::key(&a));
            for k in 2..2 + r.below(4) {
                taken.insert(oracle::name::key(&format!("{a} {k}")));
            }
        }
        tally.check("unique", seed, oracle::name::unique(&a, &taken), core::name::unique(&a, &taken));
        let f = file_name(&mut r);
        tally.check("from_file_name", seed, oracle::name::from_file_name(&f), core::name::from_file_name(&f));
        let data = bytes_of(&mut r);
        tally.check("text", seed, oracle::text(&data).map_err(|e| e.to_string()), core::text(&data).map_err(|e| e.to_string()));
    }
    tally.check("limits", 0, (oracle::name::MAX_LENGTH, oracle::MAX_FILE_SIZE), (core::name::MAX_LENGTH, core::MAX_FILE_SIZE));
    tally.finish("presets names", n);
    leaks.check("presets names");
}

// MARK: The lockstep

struct Side {
    folder: PathBuf,
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("negs-diff-presets-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A temporary file's name with its process, time and counter left out.
fn file_key(name: &str) -> String {
    let temporary = name.starts_with('.') && name.ends_with(".tmp") && name[1..name.len() - 4].chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    if temporary { ".<id>.tmp".into() } else { name.to_string() }
}

/// FILE_ATTRIBUTE_HIDDEN on Windows, UF_HIDDEN (the stand-in's) elsewhere.
#[cfg(windows)]
fn hidden(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 2 != 0
}

#[cfg(not(windows))]
fn hidden(meta: &std::fs::Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    meta.st_flags() & 0x8000 != 0
}

/// The folder as it is: each entry's name, kind, hidden flag and bytes.
fn snapshot(folder: &Path) -> Vec<(String, String, bool, Vec<u8>)> {
    let Ok(entries) = std::fs::read_dir(folder) else { return vec![("<none>".into(), String::new(), false, vec![])] };
    let mut out: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let name = file_key(&e.file_name().to_string_lossy());
            let meta = std::fs::symlink_metadata(e.path()).unwrap();
            let hidden = hidden(&meta);
            if meta.is_dir() { (name, "dir".into(), hidden, vec![]) } else { (name, "file".into(), hidden, std::fs::read(e.path()).unwrap_or_default()) }
        })
        .collect();
    out.sort();
    out
}

type Shown = Result<String, String>;

fn same_text(e: &str, folder: &Path) -> String {
    e.replace(&folder.display().to_string(), "<folder>")
}

fn show_oracle(r: anyhow::Result<oracle::PresetFile>, folder: &Path) -> Shown {
    r.map(|f| format!("{} {}", f.path.strip_prefix(folder).map(|p| p.display().to_string()).unwrap_or_else(|_| f.path.display().to_string()), f.size))
        .map_err(|e| same_text(&format!("{e:#}"), folder))
}

fn show_core(r: anyhow::Result<core::PresetFile>, folder: &Path) -> Shown {
    r.map(|f| format!("{} {}", f.path.strip_prefix(folder).map(|p| p.display().to_string()).unwrap_or_else(|_| f.path.display().to_string()), f.size))
        .map_err(|e| same_text(&format!("{e:#}"), folder))
}

fn shown_unit<T>(r: anyhow::Result<T>, folder: &Path, f: impl Fn(T) -> String) -> Shown {
    r.map(f).map_err(|e| same_text(&format!("{e:#}"), folder))
}

/// Both sides' handles on one preset, as each listed it.
#[derive(Clone)]
struct Pair(oracle::PresetFile, core::PresetFile);

fn hide(path: &Path) {
    negs_state_oracle::platform::hide_file(path);
}

/// A DOS device name, also with an extension, on Windows before 11
/// ("CON.json" is the console there, "COM1.txt" a serial port): another
/// program can't make such a file, and the steps that open a path
/// directly would read the console and wait for ever, so they skip them
/// there. The libraries' own calls refuse these names (`name::problem`).
fn device(file: &str) -> bool {
    let stem = file.split('.').next().unwrap_or("").trim_end_matches(' ').to_uppercase();
    let numbered = |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| ["1", "2", "3", "4", "5", "6", "7", "8", "9", "\u{B9}", "\u{B2}", "\u{B3}"].contains(&n));
    cfg!(windows) && (["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str()) || numbered("COM") || numbered("LPT"))
}

#[test]
fn lockstep_folders_stay_alike() {
    let _serial = serial();
    register();
    let leaks = Leaks::start();
    let root = scratch("lockstep");
    let (a, b) = (Side { folder: root.join("a").join("Presets") }, Side { folder: root.join("b").join("Presets") });
    let mut tally = Tally::default();
    let runs = (cases() / 100).max(1);
    let steps = 100;
    let mut succeeded: std::collections::BTreeMap<&'static str, (u64, u64)> = Default::default();
    let gui = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/presets/spektrafilm-gui-default-28bf883.json")).unwrap();
    for seed in first_seed()..first_seed() + runs {
        let _ = std::fs::remove_dir_all(&root);
        let oracle_library = oracle::Library { folder: a.folder.clone() };
        let core_library = core::Library { folder: b.folder.clone() };
        let mut r = Random(seed);
        let mut held: Vec<Pair> = vec![];
        for step in 0..steps {
            let mut note = |what: &'static str, shown: &Shown| *succeeded.entry(what).or_insert((0, 0)) = {
                let (ok, all) = succeeded.get(what).copied().unwrap_or((0, 0));
                (ok + shown.is_ok() as u64, all + 1)
            };
            let at = seed * 1000 + step;
            let pick = |r: &mut Random, held: &[Pair]| (!held.is_empty()).then(|| held[r.index(held.len())].clone());
            match r.below(16) {
                0..=2 => {
                    let data = if r.chance(5) { gui.clone() } else { bytes_of(&mut r) };
                    let n = name(&mut r);
                    let o = oracle_library.add(&data, &n);
                    let c = core_library.add(&data, &n);
                    if let (Ok(x), Ok(y)) = (&o, &c) {
                        held.push(Pair(x.clone(), y.clone()));
                    }
                    let (o, c) = (show_oracle(o, &a.folder), show_core(c, &b.folder));
                    note("add", &o);
                    tally.check("add", at, o, c);
                }
                3 | 4 => {
                    if let Some(Pair(x, y)) = pick(&mut r, &held) {
                        let n = name(&mut r);
                        let o = oracle_library.rename(&x, &n);
                        let c = core_library.rename(&y, &n);
                        if let (Ok(x), Ok(y)) = (&o, &c) {
                            held.push(Pair(x.clone(), y.clone()));
                        }
                        let (o, c) = (show_oracle(o, &a.folder), show_core(c, &b.folder));
                    note("rename", &o);
                    tally.check("rename", at, o, c);
                    }
                }
                5 => {
                    if let Some(Pair(x, y)) = pick(&mut r, &held) {
                        tally.check("delete", at, shown_unit(oracle_library.delete(&x), &a.folder, |_| String::new()), shown_unit(core_library.delete(&y), &b.folder, |_| String::new()));
                    }
                }
                6 => {
                    if let Some(Pair(x, y)) = pick(&mut r, &held) {
                        let show = |d: Vec<u8>| format!("{d:?}");
                        tally.check("read", at, shown_unit(oracle_library.read(&x), &a.folder, show), shown_unit(core_library.read(&y), &b.folder, show));
                    }
                }
                7 => {
                    let mut chosen: Vec<Pair> = (0..r.index(held.len() + 2)).filter_map(|_| pick(&mut r, &held)).collect();
                    chosen.dedup_by(|p, q| p.0 == q.0);
                    let xs: Vec<_> = chosen.iter().map(|p| p.0.clone()).collect();
                    let ys: Vec<_> = chosen.iter().map(|p| p.1.clone()).collect();
                    tally.check("set_order", at, shown_unit(oracle_library.set_order(&xs), &a.folder, |_| String::new()), shown_unit(core_library.set_order(&ys), &b.folder, |_| String::new()));
                }
                8 | 9 => {
                    let o = oracle_library.list();
                    let c = core_library.list();
                    if let (Ok(xs), Ok(ys)) = (&o, &c) {
                        if xs.len() == ys.len() {
                            held.extend(xs.iter().cloned().zip(ys.iter().cloned()).map(|(x, y)| Pair(x, y)));
                        }
                    }
                    let show_o = |v: Vec<oracle::PresetFile>| v.into_iter().map(|f| show_oracle(Ok(f), &a.folder).unwrap()).collect::<Vec<_>>().join("|");
                    let show_c = |v: Vec<core::PresetFile>| v.into_iter().map(|f| show_core(Ok(f), &b.folder).unwrap()).collect::<Vec<_>>().join("|");
                    tally.check("list", at, shown_unit(o, &a.folder, show_o), shown_unit(c, &b.folder, show_c));
                }
                10 => {
                    // Another program copies a file in (or a folder, or hides it).
                    let n = match r.below(5) {
                        0 => format!("{}.txt", name(&mut r)),
                        1 => format!(".{}.json", name(&mut r)),
                        2 => format!("{}.JSON", name(&mut r)),
                        _ => format!("{}.json", name(&mut r)),
                    };
                    if n.contains('/') || n.contains('\0') || device(&n) {
                        continue;
                    }
                    let data = bytes_of(&mut r);
                    let kind = r.below(6);
                    for side in [&a, &b] {
                        let _ = std::fs::create_dir_all(&side.folder);
                        let p = side.folder.join(&n);
                        match kind {
                            0 => {
                                let _ = std::fs::create_dir(&p);
                            }
                            1 => {
                                let _ = std::fs::write(&p, &data);
                                hide(&p);
                            }
                            2 => {
                                let _ = std::fs::write(&p, vec![b'{'; oracle::MAX_FILE_SIZE as usize + 1]);
                            }
                            _ => {
                                let _ = std::fs::write(&p, &data);
                            }
                        }
                    }
                }
                11 => {
                    // Another program edits or removes a listed preset.
                    if let Some(Pair(x, y)) = pick(&mut r, &held) {
                        let data = bytes_of(&mut r);
                        let remove = r.chance(3);
                        for p in [&x.path, &y.path] {
                            if remove {
                                let _ = std::fs::remove_file(p);
                            } else if p.exists() {
                                let _ = std::fs::write(p, &data);
                            }
                        }
                    }
                }
                12 => {
                    // The order file, written by hand (or by a Mac).
                    let names: Vec<String> = (0..r.index(5)).map(|_| if r.chance(3) { name(&mut r) } else { format!("{}.json", name(&mut r)) }).collect();
                    let text = match r.below(4) {
                        0 => mutate(&mut r, &serde_json::to_vec(&serde_json::json!({"order": names})).unwrap()),
                        1 => serde_json::to_vec(&serde_json::json!({"order": [names.first().cloned().unwrap_or_default(), 3, null]})).unwrap(),
                        2 => json_text(&mut r),
                        _ => serde_json::to_vec(&serde_json::json!({"order": names})).unwrap(),
                    };
                    for side in [&a, &b] {
                        let _ = std::fs::create_dir_all(&side.folder);
                        let _ = std::fs::write(side.folder.join(".order.json"), &text);
                    }
                }
                13 => {
                    // read_file: a preset, too large, a folder, missing.
                    let n = format!("{}.json", name(&mut r));
                    if !n.contains('/') && !n.contains('\0') && !device(&n) {
                        let show = |d: Vec<u8>| format!("{d:?}");
                        tally.check(
                            "read_file",
                            at,
                            shown_unit(oracle::read_file(&a.folder.join(&n)), &a.folder, show),
                            shown_unit(core::read_file(&b.folder.join(&n)), &b.folder, show),
                        );
                    }
                }
                14 => {
                    if r.chance(8) {
                        let _ = std::fs::remove_dir_all(&a.folder);
                        let _ = std::fs::remove_dir_all(&b.folder);
                    }
                }
                _ => {
                    if r.chance(4) {
                        held.clear();
                    }
                }
            }
            if held.len() > 24 {
                held.drain(..held.len() - 24);
            }
            tally.check("folder", at, snapshot(&a.folder), snapshot(&b.folder));
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    eprintln!("presets lockstep: succeeded/tried {succeeded:?}");
    tally.finish("presets lockstep", runs * steps);
    leaks.check("presets lockstep");
}

// MARK: Preset JSON

/// The GUI default and a random corpus: added and read back by both, then
/// written as serde's pretty writer does (the engine reads the text; the
/// core's value package writes it as serde does).
#[test]
fn preset_json_round_trips() {
    let _serial = serial();
    register();
    let leaks = Leaks::start();
    let root = scratch("json");
    let oracle_library = oracle::Library { folder: root.join("a") };
    let core_library = core::Library { folder: root.join("b") };
    let gui = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/presets/spektrafilm-gui-default-28bf883.json")).unwrap();
    let mut tally = Tally::default();
    let n = cases() / 10;
    for seed in first_seed()..first_seed() + n {
        let mut r = Random(seed);
        let data = if seed == first_seed() { gui.clone() } else { serde_json::to_vec_pretty(&value(&mut r, 5)).unwrap() };
        let x = oracle_library.add(&data, "Corpus").unwrap();
        let y = core_library.add(&data, "Corpus").unwrap();
        let (dx, dy) = (oracle_library.read(&x).unwrap(), core_library.read(&y).unwrap());
        tally.check("bytes", seed, dx.clone(), dy.clone());
        let tx = oracle::text(&dx).unwrap();
        let ty = core::text(&dy).unwrap();
        let v: serde_json::Value = serde_json::from_str(&tx).unwrap();
        let pretty = serde_json::to_vec_pretty(&v).unwrap();
        tally.check("pretty", seed, Some(pretty.clone()), rewrite(ty.as_bytes(), negs_core_sys::state::NEGS_JSON_PRETTY));
        oracle_library.delete(&x).unwrap();
        core_library.delete(&y).unwrap();
    }
    let _ = std::fs::remove_dir_all(&root);
    tally.finish("presets json", n);
    leaks.check("presets json");
}
