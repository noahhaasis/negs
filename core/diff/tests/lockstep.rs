//! The library in lockstep: the Rust oracle (crates/negs-library) and the
//! Odin core (through negs-core-library) each get a folder made the same
//! way and the same random actions, seeded as `state.rs` is. After every
//! step both must show the same: every file's bytes (photographs, sidecars
//! and the bin), the items and what is shown, each item's name, summary,
//! look, thumbnail and export edits, the open photograph and its edits,
//! what is copied, the defaults, Undo and Redo, and each action's outcome,
//! with paths relative to the folder.
//!
//! NEGS_LIBRARY_SEEDS (default 3) runs, NEGS_LIBRARY_STEPS (default 60)
//! actions each, NEGS_LIBRARY_SEED a single seed.

use negs_library_rust::delete::{DeletePlan, Question};
use negs_library_rust::sidecar::{self, Patch, Sidecar, LATEST_DEFAULTS};
use negs_library_rust::{
    BrightnessMatch, Config, CopiedEdits, EditDefaults, EditGroup, EditState, EditsFilter, ExportEdits, Filter, Flag, FlagFilter, FolderBin, Item,
    Json, Opened, Outcome, PhotoFile, RatingFilter, Summary,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

type Rust = negs_library_rust::Library;
type Core = negs_library::Library;

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/sidecar-fixtures")
}

/// SplitMix64, as in state.rs.
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

/// The library calls the test makes, on either implementation.
trait Lib {
    fn files(&self) -> Vec<PhotoFile>;
    fn items(&self) -> Vec<Item>;
    fn shown(&self, current: Option<&Item>) -> Vec<Item>;
    fn rejected(&self) -> Vec<Item>;
    fn name(&self, item: &Item) -> String;
    fn duplicates(&self, photo: &Path) -> Vec<String>;
    fn companion(&self, photo: &Path) -> Option<PathBuf>;
    fn summary(&self, item: &Item) -> Summary;
    fn look(&self, item: &Item) -> Option<String>;
    fn thumbnail_state(&self, item: &Item) -> Option<(EditState, bool)>;
    fn export_edits(&self, item: &Item) -> Option<ExportEdits>;
    fn store_solved_match(&mut self, item: &Item, look: &str, state: &EditState) -> bool;
    fn unedited_state(&self) -> EditState;
    fn read_only(&self) -> bool;
    fn failed(&self) -> Option<String>;
    fn filter(&self) -> Filter;
    fn set_filter(&mut self, filter: Filter);
    fn defaults(&self) -> EditDefaults;
    fn copied(&self) -> Option<CopiedEdits>;
    fn previous_edits(&self) -> Option<EditState>;
    fn undo_name(&self) -> Option<String>;
    fn redo_name(&self) -> Option<String>;
    fn undo_depth(&self) -> (usize, usize);
    fn set_defaults(&mut self, defaults: EditDefaults) -> Outcome;
    fn set_pairing(&mut self, pairing: bool);
    fn set_sidecars(&mut self, on: bool);
    fn refresh(&mut self, interacting: bool) -> (Vec<PathBuf>, Outcome);
    fn open_item(&self) -> Option<Item>;
    fn state(&self) -> Option<EditState>;
    fn open_edited(&self) -> bool;
    fn unsaved(&self) -> bool;
    fn open(&mut self, item: &Item) -> Opened;
    fn close(&mut self) -> Result<(), String>;
    fn edit(&mut self, state: EditState);
    fn save(&mut self) -> Result<(), String>;
    fn set_rating(&mut self, items: &[Item], stars: i64) -> Outcome;
    fn change_rating(&mut self, items: &[Item], by: i64) -> Outcome;
    fn set_flag(&mut self, items: &[Item], flag: Option<Flag>) -> Outcome;
    fn copy_edits(&mut self, item: &Item, groups: BTreeSet<EditGroup>) -> Outcome;
    fn paste_edits(&mut self, items: &[Item]) -> Outcome;
    fn paste_from_previous(&mut self, items: &[Item]) -> Outcome;
    fn reset_edits(&mut self, items: &[Item]) -> Outcome;
    fn change_each_edit(&mut self, items: &[Item], action: &str, note: &str, transform: &dyn Fn(&Item, &EditState) -> EditState) -> Outcome;
    fn opening_state(&mut self, item: &Item) -> Option<EditState>;
    fn new_id(&mut self) -> String;
    fn duplicate(&mut self, items: &[Item]) -> Outcome;
    fn delete_plan(&self, items: &[Item]) -> DeletePlan;
    fn delete_question(&self, plan: &DeletePlan) -> Question;
    fn delete_is_permanent(&self, plan: &DeletePlan) -> bool;
    fn delete(&mut self, plan: &DeletePlan) -> Outcome;
    fn undo(&mut self) -> Option<Outcome>;
    fn redo(&mut self) -> Option<Outcome>;
}

macro_rules! impl_lib {
    ($t:ty) => {
        impl Lib for $t {
            fn files(&self) -> Vec<PhotoFile> { <$t>::files(self).to_vec() }
            fn items(&self) -> Vec<Item> { <$t>::items(self) }
            fn shown(&self, current: Option<&Item>) -> Vec<Item> { <$t>::shown(self, current) }
            fn rejected(&self) -> Vec<Item> { <$t>::rejected(self) }
            fn name(&self, item: &Item) -> String { <$t>::name(self, item) }
            fn duplicates(&self, photo: &Path) -> Vec<String> { <$t>::duplicates(self, photo).to_vec() }
            fn companion(&self, photo: &Path) -> Option<PathBuf> { <$t>::companion(self, photo).map(Path::to_path_buf) }
            fn summary(&self, item: &Item) -> Summary { <$t>::summary(self, item) }
            fn look(&self, item: &Item) -> Option<String> { <$t>::look(self, item).map(str::to_string) }
            fn thumbnail_state(&self, item: &Item) -> Option<(EditState, bool)> { <$t>::thumbnail_state(self, item) }
            fn export_edits(&self, item: &Item) -> Option<ExportEdits> { <$t>::export_edits(self, item) }
            fn store_solved_match(&mut self, item: &Item, look: &str, state: &EditState) -> bool { <$t>::store_solved_match(self, item, look, state) }
            fn unedited_state(&self) -> EditState { <$t>::unedited_state(self) }
            fn read_only(&self) -> bool { <$t>::read_only(self) }
            fn failed(&self) -> Option<String> { <$t>::failed(self).map(str::to_string) }
            fn filter(&self) -> Filter { <$t>::filter(self).clone() }
            fn set_filter(&mut self, filter: Filter) { <$t>::set_filter(self, filter) }
            fn defaults(&self) -> EditDefaults { <$t>::defaults(self).clone() }
            fn copied(&self) -> Option<CopiedEdits> { <$t>::copied(self).cloned() }
            fn previous_edits(&self) -> Option<EditState> { <$t>::previous_edits(self).cloned() }
            fn undo_name(&self) -> Option<String> { <$t>::undo_name(self).map(str::to_string) }
            fn redo_name(&self) -> Option<String> { <$t>::redo_name(self).map(str::to_string) }
            fn undo_depth(&self) -> (usize, usize) { <$t>::undo_depth(self) }
            fn set_defaults(&mut self, defaults: EditDefaults) -> Outcome { <$t>::set_defaults(self, defaults) }
            fn set_pairing(&mut self, pairing: bool) { <$t>::set_pairing(self, pairing) }
            fn set_sidecars(&mut self, on: bool) { <$t>::set_sidecars(self, on) }
            fn refresh(&mut self, interacting: bool) -> (Vec<PathBuf>, Outcome) { <$t>::refresh(self, interacting) }
            fn open_item(&self) -> Option<Item> { <$t>::open_item(self).cloned() }
            fn state(&self) -> Option<EditState> { <$t>::state(self).cloned() }
            fn open_edited(&self) -> bool { <$t>::open_edited(self) }
            fn unsaved(&self) -> bool { <$t>::unsaved(self) }
            fn open(&mut self, item: &Item) -> Opened { <$t>::open(self, item) }
            fn close(&mut self) -> Result<(), String> { <$t>::close(self) }
            fn edit(&mut self, state: EditState) { <$t>::edit(self, state) }
            fn save(&mut self) -> Result<(), String> { <$t>::save(self) }
            fn set_rating(&mut self, items: &[Item], stars: i64) -> Outcome { <$t>::set_rating(self, items, stars) }
            fn change_rating(&mut self, items: &[Item], by: i64) -> Outcome { <$t>::change_rating(self, items, by) }
            fn set_flag(&mut self, items: &[Item], flag: Option<Flag>) -> Outcome { <$t>::set_flag(self, items, flag) }
            fn copy_edits(&mut self, item: &Item, groups: BTreeSet<EditGroup>) -> Outcome { <$t>::copy_edits(self, item, groups) }
            fn paste_edits(&mut self, items: &[Item]) -> Outcome { <$t>::paste_edits(self, items) }
            fn paste_from_previous(&mut self, items: &[Item]) -> Outcome { <$t>::paste_from_previous(self, items) }
            fn reset_edits(&mut self, items: &[Item]) -> Outcome { <$t>::reset_edits(self, items) }
            fn change_each_edit(&mut self, items: &[Item], action: &str, note: &str, transform: &dyn Fn(&Item, &EditState) -> EditState) -> Outcome {
                <$t>::change_each_edit(self, items, action, note, transform)
            }
            fn opening_state(&mut self, item: &Item) -> Option<EditState> { <$t>::opening_state(self, item) }
            fn new_id(&mut self) -> String { <$t>::new_id(self) }
            fn duplicate(&mut self, items: &[Item]) -> Outcome { <$t>::duplicate(self, items) }
            fn delete_plan(&self, items: &[Item]) -> DeletePlan { <$t>::delete_plan(self, items) }
            fn delete_question(&self, plan: &DeletePlan) -> Question { <$t>::delete_question(self, plan) }
            fn delete_is_permanent(&self, plan: &DeletePlan) -> bool { <$t>::delete_is_permanent(self, plan) }
            fn delete(&mut self, plan: &DeletePlan) -> Outcome { <$t>::delete(self, plan) }
            fn undo(&mut self) -> Option<Outcome> { <$t>::undo(self) }
            fn redo(&mut self) -> Option<Outcome> { <$t>::redo(self) }
        }
    };
}

impl_lib!(Rust);
impl_lib!(Core);

/// One side: its library and the folder it lives in (`root/photos`,
/// `root/bin`).
struct Side {
    root: PathBuf,
}

impl Side {
    fn photos(&self) -> PathBuf {
        self.root.join("photos")
    }
    fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }
    /// A relative item ("a.png") as this side's.
    fn item(&self, rel: &Item) -> Item {
        Item { photo: self.photos().join(&rel.photo), variant: rel.variant.clone() }
    }
    fn items(&self, rel: &[Item]) -> Vec<Item> {
        rel.iter().map(|i| self.item(i)).collect()
    }
    /// A plan of the oracle's, made this side's.
    fn plan(&self, rel: &DeletePlan) -> DeletePlan {
        DeletePlan {
            items: self.items(&rel.items),
            files: rel.files.iter().map(|f| self.photos().join(f)).collect(),
            duplicates: self.items(&rel.duplicates),
            following: rel.following,
            companions: rel.companions.iter().map(|(f, c)| (self.photos().join(f), self.photos().join(c))).collect(),
            renamed: rel.renamed.iter().map(|(a, b)| (self.item(a), self.item(b))).collect(),
        }
    }
    /// Text with this side's folder written `<root>`.
    fn relative(&self, text: String) -> String {
        text.replace(&self.root.to_string_lossy().into_owned(), "<root>")
    }
}

fn relative_item(side: &Side, item: &Item) -> Item {
    Item { photo: item.photo.strip_prefix(side.photos()).map(Path::to_path_buf).unwrap_or_else(|_| item.photo.clone()), variant: item.variant.clone() }
}

fn relative_plan(side: &Side, plan: &DeletePlan) -> DeletePlan {
    let photos = side.photos();
    let rel = |p: &PathBuf| p.strip_prefix(&photos).map(Path::to_path_buf).unwrap_or_else(|_| p.clone());
    DeletePlan {
        items: plan.items.iter().map(|i| relative_item(side, i)).collect(),
        files: plan.files.iter().map(rel).collect(),
        duplicates: plan.duplicates.iter().map(|i| relative_item(side, i)).collect(),
        following: plan.following,
        companions: plan.companions.iter().map(|(f, c)| (rel(f), rel(c))).collect(),
        renamed: plan.renamed.iter().map(|(a, b)| (relative_item(side, a), relative_item(side, b))).collect(),
    }
}

/// A plan as text, its companions in order.
fn plan_text(plan: &DeletePlan) -> String {
    let companions: BTreeMap<&PathBuf, &PathBuf> = plan.companions.iter().collect();
    format!(
        "items {:?} files {:?} duplicates {:?} following {} companions {companions:?} renamed {:?}",
        plan.items, plan.files, plan.duplicates, plan.following, plan.renamed
    )
}

fn folder_bytes(folder: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries {
            let entry = entry.unwrap();
            files.insert(entry.file_name().to_string_lossy().into_owned(), std::fs::read(entry.path()).unwrap_or_default());
        }
    }
    files
}

/// Everything a side shows, as lines, paths relative.
fn view(side: &Side, lib: &dyn Lib) -> Vec<String> {
    let mut lines = Vec::new();
    let names = |items: &[Item]| items.iter().map(|i| lib.name(i)).collect::<Vec<_>>();
    let items = lib.items();
    let open = lib.open_item();
    lines.push(format!("items {:?}", names(&items)));
    lines.push(format!("shown {:?}", names(&lib.shown(None))));
    lines.push(format!("shown with the open one {:?}", names(&lib.shown(open.as_ref()))));
    lines.push(format!("rejected {:?}", names(&lib.rejected())));
    for file in lib.files() {
        lines.push(format!(
            "file {} size {:?} has a time {} duplicates {:?} companion {:?}",
            file.name,
            file.size,
            file.modified.is_some(),
            lib.duplicates(&file.path),
            lib.companion(&file.path).map(|c| c.file_name().unwrap().to_string_lossy().into_owned())
        ));
    }
    for item in &items {
        lines.push(format!("{} summary {:?} look {:?}", lib.name(item), lib.summary(item), lib.look(item)));
        lines.push(format!("{} thumbnail {:?}", lib.name(item), lib.thumbnail_state(item)));
        lines.push(format!("{} export {:?}", lib.name(item), lib.export_edits(item)));
    }
    lines.push(format!("read only {} failed {:?} filter {:?}", lib.read_only(), lib.failed(), lib.filter()));
    lines.push(format!("open {:?} edited {} unsaved {}", open.as_ref().map(|i| relative_item(side, i)), lib.open_edited(), lib.unsaved()));
    lines.push(format!("state {:?}", lib.state()));
    lines.push(format!("copied {:?}", lib.copied()));
    lines.push(format!("previous {:?}", lib.previous_edits()));
    lines.push(format!("defaults {:?}", lib.defaults()));
    lines.push(format!("unedited {:?}", lib.unedited_state()));
    lines.push(format!("undo {:?} redo {:?} depth {:?}", lib.undo_name(), lib.redo_name(), lib.undo_depth()));
    lines.into_iter().map(|l| side.relative(l)).collect()
}

fn outcome_text(side: &Side, outcome: &Outcome) -> String {
    let o = Outcome {
        open: outcome.open.as_ref().map(|i| relative_item(side, i)),
        select: outcome.select.iter().map(|i| relative_item(side, i)).collect(),
        ..outcome.clone()
    };
    side.relative(format!("{o:?}"))
}

fn new_library(core: bool, root: &Path, seed: u64, film: &str) -> Box<dyn Lib> {
    let config = Config {
        data: data(),
        app: "test".into(),
        bin: Box::new(FolderBin::new(root.join("bin"))),
        pairing: true,
        sidecars: true,
        id_seed: Some(seed),
    };
    let mut lib: Box<dyn Lib> =
        if core { Box::new(Core::new(root.join("photos"), config).unwrap()) } else { Box::new(Rust::new(root.join("photos"), config).unwrap()) };
    let mut defaults = lib.defaults();
    defaults.new_photo_film = film.into();
    lib.set_defaults(defaults);
    lib
}

/// The folder both start from: plain photographs, sidecars of each kind the
/// fixtures have (edited, duplicates, rejected, from a newer Negs), and a
/// RAW with its camera JPEG.
fn make_folder(root: &Path) {
    let photos = root.join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    for (index, name) in ["a.png", "b.png", "c.png", "d.png", "e.png", "f.CR3", "f.JPG"].iter().enumerate() {
        std::fs::write(photos.join(name), [b'p', b'n', b'g', index as u8]).unwrap();
    }
    for (photo, fixture) in [("b.png", "edited.negs"), ("c.png", "two-variants.negs"), ("d.png", "rated-reject.negs"), ("e.png", "newer-schema.negs")] {
        std::fs::copy(fixtures().join(fixture), photos.join(format!("{photo}.negs"))).unwrap();
    }
}

fn name_of(lib: &dyn Lib, side: &Side, item: &Item) -> String {
    lib.name(&side.item(item))
}

fn names_of(lib: &dyn Lib, side: &Side, list: &[Item]) -> String {
    list.iter().map(|i| name_of(lib, side, i)).collect::<Vec<_>>().join(", ")
}

struct Run {
    steps: usize,
    checks: usize,
}

/// The two sides after a step: the same files, views and outcome.
fn compare(rust: &Side, core: &Side, libs: (&dyn Lib, &dyn Lib), what: &str, outcomes: (String, String), run: &mut Run) -> Result<(), String> {
    run.checks += 1;
    let mut diffs = Vec::new();
    if outcomes.0 != outcomes.1 {
        diffs.push(format!("outcome\n      rust {}\n      core {}", outcomes.0, outcomes.1));
    }
    for (label, a, b) in [("photos", folder_bytes(&rust.photos()), folder_bytes(&core.photos())), ("bin", folder_bytes(&rust.bin()), folder_bytes(&core.bin()))] {
        let names: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
        for name in names {
            if a.get(name) != b.get(name) {
                let show = |x: Option<&Vec<u8>>| x.map(|b| String::from_utf8_lossy(b).replace('\n', " ")).unwrap_or_else(|| "absent".into());
                diffs.push(format!("{label}/{name}\n      rust {}\n      core {}", show(a.get(name)), show(b.get(name))));
            }
        }
    }
    let (a, b) = (view(rust, libs.0), view(core, libs.1));
    if a != b {
        for i in 0..a.len().max(b.len()) {
            if a.get(i) != b.get(i) {
                diffs.push(format!("view\n      rust {:?}\n      core {:?}", a.get(i), b.get(i)));
            }
        }
    }
    if diffs.is_empty() { Ok(()) } else { Err(format!("after {what}:\n    {}", diffs.join("\n    "))) }
}

fn sequence(seed: u64, steps: usize, root: &Path, run: &mut Run) -> Result<(), String> {
    let mut random = Random(seed);
    let mut new_film = "kodak_gold_200".to_string();
    let rust = Side { root: root.join("rust") };
    let core = Side { root: root.join("core") };
    let mut rlib = new_library(false, &rust.root, seed, &new_film);
    let mut clib = new_library(true, &core.root, seed, &new_film);
    let mut log: Vec<String> = Vec::new();
    let fail = |log: &[String], what: String| -> Result<(), String> {
        Err(format!("seed {seed}: {what}\n  after {}", log[log.len().saturating_sub(14)..].join("; ")))
    };
    // Both sides, one call each; the outcome texts compared after.
    macro_rules! both {
        (|$lib:ident, $side:ident| $e:expr) => {{
            let r = {
                let $side = &rust;
                let _ = $side;
                let $lib = &mut *rlib;
                $e
            };
            let c = {
                let $side = &core;
                let _ = $side;
                let $lib = &mut *clib;
                $e
            };
            (r, c)
        }};
    }
    let items0 = rlib.items();
    let first = relative_item(&rust, &items0[0]);
    both!(|lib, side| lib.open(&side.item(&first)));
    compare(&rust, &core, (&*rlib, &*clib), "opening", (String::new(), String::new()), run)?;

    for _ in 0..steps {
        run.steps += 1;
        let saved = both!(|lib, _side| lib.save());
        if saved.0 != saved.1 {
            return fail(&log, format!("save {saved:?}"));
        }
        let items: Vec<Item> = rlib.items().iter().map(|i| relative_item(&rust, i)).collect();
        let open = rlib.open_item().map(|i| relative_item(&rust, &i));
        let mut text = (String::new(), String::new());
        let what: String;
        match random.below(30) {
            0..=4 => {
                let Some(state) = rlib.state() else { continue };
                let mut next = state;
                match random.below(6) {
                    0..=3 => {
                        let path = random.pick(&SLIDERS);
                        let value = if random.chance(0.25) { 0.0 } else { (random.below(9) as f64 - 4.0) * 10.0 + random.below(3) as f64 * 0.125 };
                        what = format!("{path} {value}");
                        next.params.set_at(path, Json::Number(value));
                    }
                    4 => {
                        let film = random.pick(&FILMS);
                        what = format!("film {film}");
                        next.film = film.into();
                    }
                    _ => {
                        let ev = (random.below(9) as f64 - 4.0) * 0.25;
                        what = format!("brightness match {ev}");
                        next.brightness_match = Some(BrightnessMatch { ev: Some(ev), highlight_limited: random.chance(0.3), solved: random.chance(0.5) });
                    }
                }
                both!(|lib, _side| lib.edit(next.clone()));
            }
            5 | 6 => {
                let item = random.pick(&items);
                what = format!("open {}", name_of(&*rlib, &rust, &item));
                let o = both!(|lib, side| lib.open(&side.item(&item)));
                text = (format!("{:?}", o.0), format!("{:?}", o.1));
            }
            7 | 8 => {
                let targets = if random.chance(0.5) || open.is_none() { random.subset(&items) } else { vec![open.clone().unwrap()] };
                let o = if random.chance(0.6) {
                    let stars = random.below(7) as i64 - if random.chance(0.1) { 3 } else { 0 };
                    what = format!("rate {} {stars}", names_of(&*rlib, &rust, &targets));
                    both!(|lib, side| lib.set_rating(&side.items(&targets), stars))
                } else if random.chance(0.3) {
                    let by = if random.chance(0.5) { 1 } else { -1 };
                    what = format!("rating {by:+} {}", names_of(&*rlib, &rust, &targets));
                    both!(|lib, side| lib.change_rating(&side.items(&targets), by))
                } else {
                    let flag = random.pick(&[None, Some(Flag::Pick), Some(Flag::Reject)]);
                    what = format!("flag {} {flag:?}", names_of(&*rlib, &rust, &targets));
                    both!(|lib, side| lib.set_flag(&side.items(&targets), flag))
                };
                text = (outcome_text(&rust, &o.0), outcome_text(&core, &o.1));
            }
            9 => {
                let item = random.pick(&items);
                what = format!("copy {}", name_of(&*rlib, &rust, &item));
                let groups: BTreeSet<EditGroup> = match random.below(3) {
                    0 => EditGroup::standard(),
                    1 => EditGroup::ALL.into_iter().collect(),
                    _ => EditGroup::ALL.into_iter().filter(|_| random.chance(0.5)).collect(),
                };
                let o = both!(|lib, side| lib.copy_edits(&side.item(&item), groups.clone()));
                text = (outcome_text(&rust, &o.0), outcome_text(&core, &o.1));
            }
            10 | 11 => {
                let targets = random.subset(&items);
                what = format!("paste onto {}", names_of(&*rlib, &rust, &targets));
                let o = both!(|lib, side| lib.paste_edits(&side.items(&targets)));
                text = (outcome_text(&rust, &o.0), outcome_text(&core, &o.1));
            }
            12 => {
                let targets = random.subset(&items);
                what = format!("paste previous onto {}", names_of(&*rlib, &rust, &targets));
                let o = both!(|lib, side| lib.paste_from_previous(&side.items(&targets)));
                text = (outcome_text(&rust, &o.0), outcome_text(&core, &o.1));
            }
            13 => {
                let targets = random.subset(&items);
                what = format!("reset {}", names_of(&*rlib, &rust, &targets));
                let o = both!(|lib, side| lib.reset_edits(&side.items(&targets)));
                text = (outcome_text(&rust, &o.0), outcome_text(&core, &o.1));
            }
            14 => {
                if items.len() >= 12 {
                    continue;
                }
                let targets = random.subset(&items);
                what = format!("duplicate {}", names_of(&*rlib, &rust, &targets));
                let o = both!(|lib, side| lib.duplicate(&side.items(&targets)));
                text = (outcome_text(&rust, &o.0), outcome_text(&core, &o.1));
            }
            15 | 16 => {
                let targets = if random.chance(0.2) {
                    rlib.rejected().iter().map(|i| relative_item(&rust, i)).collect()
                } else {
                    random.subset(&items)
                };
                let photos: BTreeSet<&PathBuf> = targets.iter().map(|i| &i.photo).collect();
                if targets.is_empty() || photos.len() + 1 >= rlib.files().len() {
                    continue;
                }
                what = format!("delete {}", names_of(&*rlib, &rust, &targets));
                let plans = both!(|lib, side| relative_plan(side, &lib.delete_plan(&side.items(&targets))));
                if plan_text(&plans.0) != plan_text(&plans.1) {
                    return fail(&log, format!("delete plan\n      rust {}\n      core {}", plan_text(&plans.0), plan_text(&plans.1)));
                }
                let plan = plans.0;
                let asked = both!(|lib, side| (lib.delete_question(&side.plan(&plan)), lib.delete_is_permanent(&side.plan(&plan))));
                if asked.0 != asked.1 {
                    return fail(&log, format!("delete question {asked:?}"));
                }
                let o = both!(|lib, side| lib.delete(&side.plan(&plan)));
                text = (outcome_text(&rust, &o.0), outcome_text(&core, &o.1));
                if let Some(item) = o.0.open.as_ref().map(|i| relative_item(&rust, i)) {
                    both!(|lib, side| lib.open(&side.item(&item)));
                }
            }
            17 | 18 => {
                let undo = random.chance(0.6);
                what = if undo { format!("undo {:?}", rlib.undo_name()) } else { format!("redo {:?}", rlib.redo_name()) };
                let o = both!(|lib, _side| if undo { lib.undo() } else { lib.redo() });
                text = (
                    o.0.as_ref().map(|x| outcome_text(&rust, x)).unwrap_or_else(|| "none".into()),
                    o.1.as_ref().map(|x| outcome_text(&core, x)).unwrap_or_else(|| "none".into()),
                );
                if let Some(item) = o.0.and_then(|x| x.open).map(|i| relative_item(&rust, &i)) {
                    both!(|lib, side| lib.open(&side.item(&item)));
                }
            }
            19 => {
                if open.is_some() {
                    what = "grid".into();
                    let o = both!(|lib, _side| lib.close());
                    text = (format!("{:?}", o.0), format!("{:?}", o.1));
                } else {
                    let item = random.pick(&items);
                    what = format!("edit view {}", name_of(&*rlib, &rust, &item));
                    let o = both!(|lib, side| lib.open(&side.item(&item)));
                    text = (format!("{:?}", o.0), format!("{:?}", o.1));
                }
            }
            20 => {
                // Another app changes a sidecar: the same bytes on both sides.
                both!(|lib, _side| lib.save()).0.ok();
                let files: Vec<String> = rlib.files().iter().map(|f| f.name.clone()).collect();
                let file = random.pick(&files);
                let photo = rust.photos().join(&file);
                let raw = sidecar::read_raw(&photo).ok().flatten();
                let version = raw.clone().and_then(|r| Sidecar::from_raw(r).ok()).map(|s| s.defaults_version).unwrap_or(LATEST_DEFAULTS);
                let mut patch = Patch { primary: true, ..Patch::new("other", version, "other") };
                if random.chance(0.5) {
                    let stars = random.below(6) as i64;
                    patch.rating = Some(stars);
                    what = format!("another app rates {file} {stars}");
                } else {
                    let path = random.pick(&SLIDERS);
                    let value = (random.below(9) as f64 - 4.0) * 10.0;
                    let (section, key) = path.split_once('.').unwrap();
                    patch.edits = Some(Some(Json::from_pairs([
                        ("film", Json::string(random.pick(&FILMS))),
                        ("paper", Json::string("kodak_portra_endura")),
                        ("params", Json::from_pairs([(section, Json::from_pairs([(key, Json::Number(value))]))])),
                    ])));
                    what = format!("another app sets {file} {path} {value}");
                }
                let bad = random.chance(0.1);
                if let Ok(changed) = patch.applied(raw.as_ref()) {
                    for side in [&rust, &core] {
                        let path = sidecar::sidecar_path(&side.photos().join(&file));
                        match (&changed, bad) {
                            (_, true) => std::fs::write(&path, b"{ not json").unwrap(),
                            (Some(json), false) => std::fs::write(&path, json.to_sidecar_bytes()).unwrap(),
                            (None, false) => {
                                let _ = std::fs::remove_file(&path);
                            }
                        }
                    }
                }
                let o = both!(|lib, side| {
                    let (changed, outcome) = lib.refresh(false);
                    let changed: Vec<String> = changed.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
                    format!("{changed:?} {}", outcome_text(side, &outcome))
                });
                text = o;
            }
            21 => {
                let others: Vec<&str> = FILMS.iter().copied().filter(|f| *f != new_film).collect();
                new_film = random.pick(&others).to_string();
                what = format!("new photographs' film {new_film}");
                let mut defaults = rlib.defaults();
                defaults.new_photo_film = new_film.clone();
                defaults.match_new_photos = random.chance(0.5);
                let o = both!(|lib, side| lib.set_defaults(defaults.clone()));
                text = (outcome_text(&rust, &o.0), outcome_text(&core, &o.1));
            }
            22 => {
                what = "quit and relaunch".into();
                let closed = both!(|lib, _side| lib.close());
                if closed.0 != closed.1 {
                    return fail(&log, format!("close {closed:?}"));
                }
                rlib = new_library(false, &rust.root, seed.wrapping_add(1), &new_film);
                clib = new_library(true, &core.root, seed.wrapping_add(1), &new_film);
                if let Some(item) = open.as_ref().filter(|i| rust.photos().join(&i.photo).exists()) {
                    both!(|lib, side| lib.open(&side.item(item)));
                }
            }
            23 => {
                let targets = random.subset(&items);
                let delta = (random.below(5) as f64 - 2.0) * 5.0;
                what = format!("change each edit of {} by {delta}", names_of(&*rlib, &rust, &targets));
                let transform = move |item: &Item, state: &EditState| {
                    let mut next = state.clone();
                    let base = next.params.at("prep.contrast").as_f64().unwrap_or(0.0);
                    next.params.set_at("prep.contrast", Json::Number(base + delta + item.file_name().len() as f64));
                    next
                };
                let o = both!(|lib, side| lib.change_each_edit(&side.items(&targets), "Contrast", "Contrast Changed", &transform));
                text = (outcome_text(&rust, &o.0), outcome_text(&core, &o.1));
            }
            24 => {
                let rating = match random.below(4) {
                    0 => RatingFilter::Any,
                    1 => RatingFilter::Unrated,
                    _ => RatingFilter::AtLeast(1 + random.below(5) as u8),
                };
                let flags: BTreeSet<FlagFilter> = [FlagFilter::Pick, FlagFilter::Unflagged, FlagFilter::Reject].into_iter().filter(|_| random.chance(0.3)).collect();
                let edits = random.pick(&[EditsFilter::Any, EditsFilter::Edited, EditsFilter::Unedited]);
                let filter = Filter { rating, flags, edits };
                what = format!("filter {filter:?}");
                both!(|lib, _side| lib.set_filter(filter.clone()));
            }
            25 => {
                let pairing = random.chance(0.5);
                what = format!("pairing {pairing}");
                both!(|lib, _side| lib.set_pairing(pairing));
            }
            26 => {
                let on = random.chance(0.7);
                what = format!("sidecars {on}");
                both!(|lib, _side| lib.set_sidecars(on));
            }
            27 => {
                let item = random.pick(&items);
                what = format!("opening state of {} and a new id", name_of(&*rlib, &rust, &item));
                let o = both!(|lib, side| format!("{:?} {}", lib.opening_state(&side.item(&item)), lib.new_id()));
                text = o;
            }
            28 => {
                // A batch solved a match for an item's stored edits.
                let item = random.pick(&items);
                what = format!("store a solved match for {}", name_of(&*rlib, &rust, &item));
                let Some(export) = rlib.export_edits(&rust.item(&item)) else { continue };
                let look = if random.chance(0.8) { export.look.clone() } else { "stale".into() };
                let mut state = export.state.clone();
                state.brightness_match = Some(BrightnessMatch { ev: Some(0.5), highlight_limited: false, solved: random.chance(0.9) });
                let o = both!(|lib, side| lib.store_solved_match(&side.item(&item), &look, &state));
                text = (format!("{}", o.0), format!("{}", o.1));
            }
            _ => {
                let interacting = random.chance(0.5);
                what = format!("refresh interacting {interacting}");
                let o = both!(|lib, side| {
                    let (changed, outcome) = lib.refresh(interacting);
                    let changed: Vec<String> = changed.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
                    format!("{changed:?} {}", outcome_text(side, &outcome))
                });
                text = o;
            }
        }
        log.push(what.clone());
        if let Err(e) = compare(&rust, &core, (&*rlib, &*clib), &what, text, run) {
            return fail(&log, e);
        }
        let refreshed = both!(|lib, side| {
            let saved = lib.save();
            let (changed, outcome) = lib.refresh(false);
            format!("{saved:?} {changed:?} {}", outcome_text(side, &outcome))
        });
        if let Err(e) = compare(&rust, &core, (&*rlib, &*clib), "save and refresh", refreshed, run) {
            return fail(&log, e);
        }
    }
    Ok(())
}

#[test]
fn rust_and_odin_libraries_move_in_lockstep() {
    let env = |key: &str| std::env::var(key).ok().and_then(|v| v.parse::<u64>().ok());
    let seeds: Vec<u64> = match env("NEGS_LIBRARY_SEED") {
        Some(seed) => vec![seed],
        None => (1..=env("NEGS_LIBRARY_SEEDS").unwrap_or(3)).collect(),
    };
    let steps = env("NEGS_LIBRARY_STEPS").unwrap_or(60) as usize;
    let mut run = Run { steps: 0, checks: 0 };
    let mut failures = Vec::new();
    for &seed in &seeds {
        let root = std::env::temp_dir().join(format!("negs-library-lockstep-{}-{seed}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        make_folder(&root.join("rust"));
        make_folder(&root.join("core"));
        if let Err(e) = sequence(seed, steps, &root, &mut run) {
            failures.push(e);
        }
        let _ = std::fs::remove_dir_all(&root);
    }
    println!("lockstep: {} seeds, {} steps, {} comparisons, {} diverged", seeds.len(), run.steps, run.checks, failures.len());
    assert!(failures.is_empty(), "{} of {} seeds diverged:\n{}", failures.len(), seeds.len(), failures.join("\n\n"));
    let stats = negs_core_sys::alloc_stats();
    if stats.tracked == 1 {
        println!("core allocations: {} live ({} bytes), {} in all, {} bad frees", stats.live_count, stats.live_bytes, stats.total_count, stats.bad_frees);
        assert_eq!((stats.live_count, stats.bad_frees), (0, 0), "the core leaked");
    }
}
