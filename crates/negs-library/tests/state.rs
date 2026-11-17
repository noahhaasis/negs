//! Random action sequences keep the sidecars and the library in step
//! (`LibraryStateTests.swift`): edits, ratings, flags, Copy and Paste,
//! Paste from Previous, Reset, Duplicate, Delete, Undo and Redo, another
//! app's writes, a new film for new photographs, and relaunches. After
//! every step the library shows what is on disk; an undoable action
//! undone at once leaves every file byte for byte as it was, and redone
//! brings back the very bytes it wrote.
//!
//! NEGS_LIBRARY_SEEDS (default 3) runs, NEGS_LIBRARY_STEPS (default 60)
//! actions each, NEGS_LIBRARY_SEED a single seed.

use negs_library::edits::{decode_edits, EditGroup};
use negs_library::sidecar::{self, Patch, Sidecar, Tables, LATEST_DEFAULTS};
use negs_library::{thumbnail, Config, EditState, Flag, FolderBin, Item, Json, Library, Outcome};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

/// SplitMix64, as in the Swift tests.
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
    fn chance(&mut self, p: f64) -> bool {
        (self.next() % 10_000) as f64 / 10_000.0 < p
    }
    fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())].clone()
    }
    fn subset<T: Clone>(&mut self, items: &[T]) -> Vec<T> {
        let some: Vec<T> = items.iter().filter(|_| self.chance(0.5)).cloned().collect();
        if some.is_empty() { vec![self.pick(items)] } else { some }
    }
}

const FILMS: [&str; 3] = ["kodak_portra_400", "kodak_ektar_100", "kodak_gold_200"];
const SLIDERS: [&str; 3] = ["prep.contrast", "prep.highlights", "prep.shadows"];

fn library(folder: &Path, bin: &Path, seed: u64, film: &str) -> Library {
    let mut library = Library::new(
        folder,
        Config { data: data(), app: "test".into(), bin: Box::new(FolderBin::new(bin)), pairing: true, sidecars: true, id_seed: Some(seed) },
    )
    .unwrap();
    let mut defaults = library.defaults().clone();
    defaults.new_photo_film = film.into();
    library.set_defaults(defaults);
    library
}

/// Every file in the folder, byte for byte, and the library as shown.
#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    files: BTreeMap<String, Vec<u8>>,
    items: Vec<String>,
    summaries: Vec<String>,
}

impl Snapshot {
    fn of(library: &Library, folder: &Path) -> Snapshot {
        let mut files = BTreeMap::new();
        for entry in std::fs::read_dir(folder).unwrap() {
            let entry = entry.unwrap();
            files.insert(entry.file_name().to_string_lossy().into_owned(), std::fs::read(entry.path()).unwrap());
        }
        let items = library.items();
        let summaries = items
            .iter()
            .map(|i| {
                let s = library.summary(i);
                format!("{} ★{} {:?}{} {:?}", library.name(i), s.rating, s.flag, if s.edited { " edited" } else { "" }, library.look(i))
            })
            .collect();
        Snapshot { files, items: items.iter().map(|i| library.name(i)).collect(), summaries }
    }

    fn difference(&self, expected: &Snapshot) -> String {
        let mut lines = Vec::new();
        let names: BTreeSet<&String> = self.files.keys().chain(expected.files.keys()).collect();
        for name in names {
            let (now, then) = (self.files.get(name), expected.files.get(name));
            if now != then {
                let show = |b: Option<&Vec<u8>>| b.map(|b| String::from_utf8_lossy(b).replace('\n', " ")).unwrap_or_else(|| "absent".into());
                lines.push(format!("{name}: {} → {}", show(then), show(now)));
            }
        }
        if self.items != expected.items {
            lines.push(format!("items {:?} → {:?}", expected.items, self.items));
        }
        if self.summaries != expected.summaries {
            lines.push(format!("summaries {:?} → {:?}", expected.summaries, self.summaries));
        }
        lines.join("\n    ")
    }
}

/// What opening `item` shows, as `user_edits`, from its sidecar on disk;
/// `None` if its variant is gone.
fn saved_edits(library: &Library, tables: &Tables, item: &Item) -> Result<Option<EditState>, String> {
    let found = sidecar::read(&item.photo).map_err(|p| p.message(&item.file_name()))?;
    let variant = match (&found, &item.variant) {
        (None, None) => None,
        (None, Some(_)) => return Ok(None),
        (Some(s), Some(id)) => match s.variant(id) {
            Some(v) => Some(v.clone()),
            None => return Ok(None),
        },
        (Some(s), None) => s.variants.first().cloned(),
    };
    let Some(edits) = variant.and_then(|v| v.edits) else { return Ok(Some(library.defaults().plain_state().user_edits())) };
    let table = tables.get(found.map(|s| s.defaults_version).unwrap_or(LATEST_DEFAULTS))?;
    let saved = decode_edits(&edits, &table).map_err(|p| p.message(&item.file_name()))?;
    Ok(Some(library.defaults().restored(&saved.state).user_edits()))
}

fn difference(shown: &EditState, saved: &EditState) -> String {
    let mut parts = Vec::new();
    if shown.film != saved.film {
        parts.push(format!("film {} vs {}", shown.film, saved.film));
    }
    for path in SLIDERS.iter().chain(&["io.rotate_cw"]) {
        if shown.params.at(path) != saved.params.at(path) {
            parts.push(format!("{path} {:?} vs {:?}", shown.params.at(path), saved.params.at(path)));
        }
    }
    if parts.is_empty() {
        parts.push(format!("{shown:?} vs {saved:?}"));
    }
    parts.join(", ")
}

/// Once everything is written: the library shows each item's rating, flag,
/// edits, look and duplicates as on disk, the open photograph shows the
/// edits its sidecar keeps, and each photograph left since keeps the edits
/// it showed.
fn violations(library: &Library, tables: &Tables, folder: &Path, left: &HashMap<Item, EditState>) -> Vec<String> {
    let mut found = Vec::new();
    let mut on_disk: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".png"))
        .collect();
    on_disk.sort();
    let mut listed: Vec<String> = library.files().iter().map(|f| f.name.clone()).collect();
    listed.sort();
    if listed != on_disk {
        found.push(format!("Listing: {listed:?} but the folder has {on_disk:?}"));
    }
    for file in library.files() {
        let photo = &file.path;
        let sidecar: Option<Sidecar> = match sidecar::read(photo) {
            Ok(s) => s,
            Err(problem) => {
                found.push(format!("Readable: {} {}", file.name, problem.message(&file.name)));
                continue;
            }
        };
        let variants = sidecar.as_ref().map(|s| s.variants.clone()).unwrap_or_default();
        let ids: Vec<String> = variants.iter().skip(1).map(|v| v.id.clone()).collect();
        if library.duplicates(photo) != ids.as_slice() {
            found.push(format!("Duplicates: {} lists {:?} but its sidecar has {ids:?}", file.name, library.duplicates(photo)));
        }
        let version = sidecar.as_ref().map(|s| s.defaults_version).unwrap_or(LATEST_DEFAULTS);
        let slots: Vec<Option<&sidecar::Variant>> = if variants.is_empty() { vec![None] } else { variants.iter().map(Some).collect() };
        for (index, variant) in slots.into_iter().enumerate() {
            let item = if index == 0 { Item::photo(photo) } else { Item::duplicate(photo, &variant.unwrap().id) };
            let name = library.name(&item);
            let summary = library.summary(&item);
            let (rating, flag) = (variant.map(|v| v.rating).unwrap_or(0), variant.and_then(|v| v.flag));
            if summary.rating != rating || summary.flag != flag {
                found.push(format!("Summary: {name} shows ★{} {:?} but its sidecar has ★{rating} {flag:?}", summary.rating, summary.flag));
            }
            let edits = variant.and_then(|v| v.edits.as_ref());
            if summary.edited != edits.is_some() {
                found.push(format!("Edited: {name} shows edited {} but its sidecar {} edits", summary.edited, if edits.is_some() { "has" } else { "has no" }));
            }
            let look = edits.map(|e| thumbnail::look(e, version));
            if library.look(&item) != look.as_deref() {
                found.push(format!("Look: {name}'s look is {:?} but its sidecar's is {look:?}", library.look(&item)));
            }
        }
    }
    if let (Some(open), Some(state)) = (library.open_item(), library.state()) {
        if open.photo.exists() {
            match saved_edits(library, tables, open) {
                Ok(Some(saved)) if saved != state.user_edits() => {
                    found.push(format!("ShownIsSaved: {} shows edits its sidecar doesn't have: {}", library.name(open), difference(&state.user_edits(), &saved)))
                }
                Err(e) => found.push(format!("ShownIsSaved: {e}")),
                _ => {}
            }
        }
    }
    for (item, edits) in left {
        if Some(item) == library.open_item() || !item.photo.exists() {
            continue;
        }
        if let Ok(Some(saved)) = saved_edits(library, tables, item) {
            if saved != *edits {
                found.push(format!("LeftIsSaved: {} was left showing edits its sidecar doesn't have: {}", library.name(item), difference(edits, &saved)));
            }
        }
    }
    found
}

/// What the front end does with an outcome: open what it says.
fn follow(library: &mut Library, outcome: Option<Outcome>) {
    let Some(outcome) = outcome else { return };
    if let Some(item) = outcome.open {
        library.open(&item);
    }
}

fn run(seed: u64, steps: usize) -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("negs-library-state-{}-{seed}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (folder, bin) = (root.join("photos"), root.join("bin"));
    std::fs::create_dir_all(&folder).unwrap();
    for (index, name) in ["a.png", "b.png", "c.png", "d.png"].iter().enumerate() {
        std::fs::write(folder.join(name), [b'p', b'n', b'g', index as u8]).unwrap();
    }
    let result = sequence(seed, steps, &folder, &bin);
    let _ = std::fs::remove_dir_all(&root);
    result
}

fn sequence(seed: u64, steps: usize, folder: &Path, bin: &Path) -> Result<(), String> {
    let tables = Tables::new(data());
    let mut random = Random(seed);
    let mut new_film = "kodak_gold_200".to_string();
    let mut library = library(folder, bin, seed, &new_film);
    let first = library.items()[0].clone();
    library.open(&first);
    let mut log: Vec<String> = Vec::new();
    let mut left: HashMap<Item, EditState> = HashMap::new();
    let fail = |log: &[String], what: String| -> Result<(), String> {
        Err(format!("seed {seed}: {what}\n  after {}", log[log.len().saturating_sub(14)..].join("; ")))
    };

    for _ in 0..steps {
        let check_undo = random.chance(0.35);
        library.save().map_err(|e| format!("seed {seed}: save: {e}"))?;
        library.refresh(false);
        let before = Snapshot::of(&library, folder);
        let items = library.items();
        let open = library.open_item().cloned();
        let leaving = |library: &Library, left: &mut HashMap<Item, EditState>| {
            if let (Some(item), Some(state)) = (library.open_item(), library.state()) {
                left.insert(item.clone(), state.user_edits());
            }
        };
        let forget = |left: &mut HashMap<Item, EditState>, items: &[Item]| {
            for item in items {
                left.remove(item);
                left.remove(&Item::photo(&item.photo));
                if item.variant.is_none() {
                    left.retain(|k, _| k.photo != item.photo);
                }
            }
        };
        let names = |library: &Library, items: &[Item]| items.iter().map(|i| library.name(i)).collect::<Vec<_>>().join(", ");
        let mut undoable: Option<String> = None;
        match random.below(21) {
            0..=4 => {
                let Some(state) = library.state().cloned() else { continue };
                let mut next = state;
                if random.below(5) < 4 {
                    let path = random.pick(&SLIDERS);
                    let value = if random.chance(0.25) { 0.0 } else { (random.below(9) as f64 - 4.0) * 10.0 };
                    log.push(format!("{path} {value}"));
                    next.params.set_at(path, Json::Number(value));
                } else {
                    let film = random.pick(&FILMS);
                    log.push(format!("film {film}"));
                    next.film = film.into();
                }
                library.edit(next);
            }
            5 | 6 => {
                let item = random.pick(&items);
                leaving(&library, &mut left);
                log.push(format!("open {}", library.name(&item)));
                library.open(&item);
            }
            7 | 8 => {
                let targets = if random.chance(0.5) || open.is_none() { random.subset(&items) } else { vec![open.clone().unwrap()] };
                if random.chance(0.6) {
                    let stars = random.below(6) as i64;
                    log.push(format!("rate {} {stars}", names(&library, &targets)));
                    library.set_rating(&targets, stars);
                    undoable = Some("rating".into());
                } else if random.chance(0.3) {
                    let by = if random.chance(0.5) { 1 } else { -1 };
                    log.push(format!("rating {by:+} {}", names(&library, &targets)));
                    library.change_rating(&targets, by);
                    undoable = Some("rating".into());
                } else {
                    let flag = random.pick(&[None, Some(Flag::Pick), Some(Flag::Reject)]);
                    log.push(format!("flag {} {flag:?}", names(&library, &targets)));
                    library.set_flag(&targets, flag);
                    undoable = Some("flag".into());
                }
            }
            9 => {
                let item = random.pick(&items);
                log.push(format!("copy {}", library.name(&item)));
                let groups = if random.chance(0.7) { EditGroup::standard() } else { EditGroup::ALL.into_iter().collect() };
                library.copy_edits(&item, groups);
            }
            10 | 11 => {
                if library.copied().is_none() {
                    continue;
                }
                let targets = random.subset(&items);
                leaving(&library, &mut left);
                log.push(format!("paste onto {}", names(&library, &targets)));
                library.paste_edits(&targets);
                forget(&mut left, &targets);
                undoable = Some("paste".into());
            }
            12 => {
                if library.previous_edits().is_none() {
                    continue;
                }
                let targets = random.subset(&items);
                leaving(&library, &mut left);
                log.push(format!("paste previous onto {}", names(&library, &targets)));
                library.paste_from_previous(&targets);
                forget(&mut left, &targets);
                undoable = Some("paste previous".into());
            }
            13 => {
                let targets = random.subset(&items);
                leaving(&library, &mut left);
                log.push(format!("reset {}", names(&library, &targets)));
                library.reset_edits(&targets);
                forget(&mut left, &targets);
                undoable = Some("reset".into());
            }
            14 => {
                if items.len() >= 9 {
                    continue;
                }
                let targets = random.subset(&items);
                leaving(&library, &mut left);
                log.push(format!("duplicate {}", names(&library, &targets)));
                library.duplicate(&targets);
                undoable = Some("duplicate".into());
            }
            15 => {
                let targets = random.subset(&items);
                let photos: BTreeSet<&PathBuf> = targets.iter().map(|i| &i.photo).collect();
                if photos.len() >= library.files().len() - 1 {
                    continue;
                }
                leaving(&library, &mut left);
                log.push(format!("delete {}", names(&library, &targets)));
                let plan = library.delete_plan(&targets);
                let outcome = library.delete(&plan);
                if let Some(error) = outcome.error {
                    return fail(&log, format!("delete: {error}"));
                }
                follow(&mut library, Some(outcome));
                forget(&mut left, &targets);
                undoable = Some("delete".into());
            }
            16 => {
                let outcome = if random.chance(0.5) {
                    let Some(name) = library.undo_name().map(str::to_string) else { continue };
                    log.push(format!("undo {name}"));
                    library.undo()
                } else {
                    let Some(name) = library.redo_name().map(str::to_string) else { continue };
                    log.push(format!("redo {name}"));
                    library.redo()
                };
                follow(&mut library, outcome);
                left.clear();
            }
            17 => {
                if library.open_item().is_some() {
                    leaving(&library, &mut left);
                    log.push("grid".into());
                    library.close().map_err(|e| format!("seed {seed}: close: {e}"))?;
                } else {
                    let item = random.pick(&items);
                    log.push(format!("edit view {}", library.name(&item)));
                    library.open(&item);
                }
            }
            18 => {
                // Another app changes a sidecar, while nothing of ours is being written.
                library.save().map_err(|e| format!("seed {seed}: save: {e}"))?;
                let files: Vec<PathBuf> = library.files().iter().map(|f| f.path.clone()).collect();
                let photo = random.pick(&files);
                let raw = sidecar::read_raw(&photo).ok().flatten();
                let version = raw.clone().and_then(|r| Sidecar::from_raw(r).ok()).map(|s| s.defaults_version).unwrap_or(LATEST_DEFAULTS);
                let mut patch = Patch { primary: true, ..Patch::new("other", version, "other") };
                let name = photo.file_name().unwrap().to_string_lossy().into_owned();
                if random.chance(0.5) {
                    let stars = random.below(6) as i64;
                    patch.rating = Some(stars);
                    log.push(format!("another app rates {name} {stars}"));
                } else {
                    let path = random.pick(&SLIDERS);
                    let value = (random.below(9) as f64 - 4.0) * 10.0;
                    let (section, key) = path.split_once('.').unwrap();
                    let edits = Json::from_pairs([
                        ("film", Json::string(random.pick(&FILMS))),
                        ("paper", Json::string("kodak_portra_endura")),
                        ("params", Json::from_pairs([(section, Json::from_pairs([(key, Json::Number(value))]))])),
                    ]);
                    patch.edits = Some(Some(edits));
                    log.push(format!("another app sets {name} {path} {value}"));
                    forget(&mut left, &[Item::photo(&photo)]);
                }
                if let Ok(changed) = patch.applied(raw.as_ref()) {
                    // Not the library's own writer: the bytes as another app might.
                    let path = sidecar::sidecar_path(&photo);
                    match changed {
                        Some(json) => std::fs::write(&path, json.to_sidecar_bytes()).unwrap(),
                        None => {
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                }
                let (_, outcome) = library.refresh(false);
                follow(&mut library, Some(outcome));
            }
            19 => {
                // Settings › New photographs: an open photograph without
                // edits follows, and opens with it from now on.
                let others: Vec<&str> = FILMS.iter().copied().filter(|f| *f != new_film).collect();
                new_film = random.pick(&others).to_string();
                log.push(format!("new photographs' film {new_film}"));
                let mut defaults = library.defaults().clone();
                defaults.new_photo_film = new_film.clone();
                library.set_defaults(defaults);
                left.clear();
            }
            _ => {
                log.push("quit and relaunch".into());
                relaunch(&mut library, folder, bin, seed, &new_film).map_err(|e| format!("seed {seed}: relaunch: {e}\n  after {}", log.join("; ")))?;
            }
        }

        // Everything written, and the folder listed again.
        library.save().map_err(|e| format!("seed {seed}: save: {e}"))?;
        let (_, outcome) = library.refresh(false);
        follow(&mut library, Some(outcome));
        let found = violations(&library, &tables, folder, &left);
        if !found.is_empty() {
            return fail(&log, found.join("\n    "));
        }

        let Some(name) = undoable.filter(|_| check_undo) else { continue };
        let after = Snapshot::of(&library, folder);
        if after == before {
            continue;
        }
        for (when, expected) in [("undo", &before), ("redo", &after)] {
            log.push(format!("{when} {name}"));
            let outcome = if when == "undo" { library.undo() } else { library.redo() };
            if outcome.is_none() {
                return fail(&log, format!("nothing to {when}"));
            }
            follow(&mut library, outcome);
            left.clear();
            library.save().map_err(|e| format!("seed {seed}: save: {e}"))?;
            library.refresh(false);
            let now = Snapshot::of(&library, folder);
            if now != *expected {
                return fail(&log, format!("{when} of {name} left\n    {}", now.difference(expected)));
            }
            let found = violations(&library, &tables, folder, &left);
            if !found.is_empty() {
                return fail(&log, found.join("\n    "));
            }
        }
    }
    relaunch(&mut library, folder, bin, seed, &new_film).map_err(|e| format!("seed {seed}: relaunch at the end: {e}\n  after {}", log.join("; ")))
}

/// Quit and open again: the library shows the same, and the photograph
/// that was open shows the same edits.
fn relaunch(library: &mut Library, folder: &Path, bin: &Path, seed: u64, film: &str) -> Result<(), String> {
    let open = library.open_item().cloned();
    let shown = library.state().map(EditState::user_edits);
    library.close()?;
    let before = Snapshot::of(library, folder);
    let mut again = self::library(folder, bin, seed.wrapping_add(1), film);
    let now = Snapshot::of(&again, folder);
    if now != before {
        return Err(now.difference(&before));
    }
    if let (Some(item), Some(shown)) = (open, shown) {
        if item.photo.exists() {
            again.open(&item);
            let reopened = again.state().unwrap().user_edits();
            if reopened != shown {
                return Err(format!("{} opens with {}", again.name(&item), difference(&reopened, &shown)));
            }
        }
    }
    *library = again;
    Ok(())
}

#[test]
fn random_action_sequences_keep_sidecars_and_the_library_in_step() {
    let seeds: Vec<u64> = match std::env::var("NEGS_LIBRARY_SEED").ok().and_then(|s| s.parse().ok()) {
        Some(seed) => vec![seed],
        None => (1..=std::env::var("NEGS_LIBRARY_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(3)).collect(),
    };
    let steps = std::env::var("NEGS_LIBRARY_STEPS").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
    let failures: Vec<String> = seeds.iter().filter_map(|&seed| run(seed, steps).err()).take(3).collect();
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
