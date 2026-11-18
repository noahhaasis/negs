//! The library's behaviours, one at a time (`LibraryTests.swift`), on
//! copies in a temporary folder.

use negs_library::edits::EditGroup;
use negs_library::sidecar::{self, sidecar_path};
use negs_library::{Config, EditState, Flag, FolderBin, Item, Json, Library};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Folder(PathBuf);

impl Folder {
    fn new(tag: &str, photos: &[&str]) -> Folder {
        let dir = std::env::temp_dir().join(format!("negs-library-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("photos")).unwrap();
        for name in photos {
            std::fs::write(dir.join("photos").join(name), name.as_bytes()).unwrap();
        }
        Folder(dir)
    }

    fn photos(&self) -> PathBuf {
        self.0.join("photos")
    }

    fn photo(&self, name: &str) -> PathBuf {
        self.photos().join(name)
    }

    fn library(&self) -> Library {
        Library::new(
            self.photos(),
            Config {
                data: root().join("data"),
                app: "test".into(),
                bin: Box::new(FolderBin::new(self.0.join("bin"))),
                pairing: true,
                sidecars: true,
                id_seed: Some(7),
            },
        )
        .unwrap()
    }

    fn sidecar(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(sidecar_path(&self.photo(name))).ok()
    }

    fn raw(&self, name: &str) -> Option<Json> {
        sidecar::read_raw(&self.photo(name)).unwrap()
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn with_contrast(state: &EditState, value: f64) -> EditState {
    let mut next = state.clone();
    next.params.set_at("prep.contrast", Json::Number(value));
    next
}

#[test]
fn ratings_and_flags_are_written_and_undone_byte_for_byte() {
    let folder = Folder::new("ratings", &["a.jpg", "b.jpg"]);
    let mut library = folder.library();
    let a = Item::photo(folder.photo("a.jpg"));
    let b = Item::photo(folder.photo("b.jpg"));
    let outcome = library.set_rating(&[a.clone()], 4);
    assert_eq!(outcome.note.unwrap().text, "★★★★");
    assert_eq!(library.summary(&a).rating, 4);
    let raw = folder.raw("a.jpg").unwrap();
    assert_eq!(raw.at("variants.0.rating").as_integer(), Some(4));
    assert_eq!(raw.get("negs").as_integer(), Some(1));
    assert_eq!(raw.get("defaultsVersion").as_integer(), Some(3));
    assert_eq!(raw.get("app").as_str(), Some("test"));
    assert!(folder.sidecar("a.jpg").unwrap().contains("\"rating\" : 4"), "the Mac's spacing");

    let note = library.change_rating(&[a.clone(), b.clone()], 1).note.unwrap();
    assert_eq!((note.text.as_str(), note.count), ("Rating Raised", 2));
    assert_eq!((library.summary(&a).rating, library.summary(&b).rating), (5, 1));
    assert_eq!(library.undo_name(), Some("Rating"));
    let before = folder.sidecar("a.jpg");
    library.set_flag(&[a.clone()], Some(Flag::Reject));
    assert_eq!(library.rejected(), [a.clone()]);
    library.undo();
    assert_eq!(folder.sidecar("a.jpg"), before);
    library.undo();
    library.undo();
    assert_eq!(folder.sidecar("a.jpg"), None, "back to no sidecar at all");
    assert_eq!(folder.sidecar("b.jpg"), None);
    assert_eq!(library.summary(&a).rating, 0);
    library.redo();
    assert_eq!(library.summary(&a).rating, 4);
    assert_eq!(folder.raw("a.jpg").unwrap().at("variants.0.rating").as_integer(), Some(4));
}

/// Undone after another app changed the same file, a rating is merged
/// with what is there now rather than writing old bytes over it.
#[test]
fn undo_merges_with_another_apps_write() {
    let folder = Folder::new("merge", &["a.jpg"]);
    let mut library = folder.library();
    let a = Item::photo(folder.photo("a.jpg"));
    library.set_rating(&[a.clone()], 2);
    let mut raw = folder.raw("a.jpg").unwrap();
    raw.set_at("variants.0.flag", Json::string("pick"));
    raw.set_at("lightroom", Json::Bool(true));
    sidecar::write_raw(&folder.photo("a.jpg"), Some(&raw)).unwrap();
    let (changed, _) = library.refresh(false);
    assert_eq!(changed, [folder.photo("a.jpg")]);
    assert_eq!(library.summary(&a).flag, Some(Flag::Pick));
    library.undo();
    let raw = folder.raw("a.jpg").unwrap();
    assert_eq!(raw.at("variants.0.rating"), &Json::Null);
    assert_eq!(raw.at("variants.0.flag").as_str(), Some("pick"));
    assert_eq!(raw.get("lightroom"), &Json::Bool(true));
}

#[test]
fn the_open_photograph_saves_only_what_changed() {
    let folder = Folder::new("open", &["a.jpg", "b.jpg"]);
    let mut library = folder.library();
    let a = Item::photo(folder.photo("a.jpg"));
    let opened = library.open(&a);
    assert_eq!(opened.problem, None);
    assert!(!library.unsaved());
    library.save().unwrap();
    assert_eq!(folder.sidecar("a.jpg"), None, "opening writes nothing");
    let state = library.state().unwrap().clone();
    library.edit(with_contrast(&state, 0.25));
    assert!(library.unsaved());
    library.save().unwrap();
    let raw = folder.raw("a.jpg").unwrap();
    assert_eq!(raw.at("variants.0.edits.params.prep.contrast").as_f64(), Some(0.25));
    assert_eq!(raw.at("variants.0.edits.film").as_str(), Some("kodak_gold_200"));
    assert!(library.look(&a).is_some());
    let (changed, _) = library.refresh(false);
    assert!(changed.is_empty(), "its own write isn't another app's");
    // Back where it started: the edits go again.
    library.edit(state.clone());
    library.save().unwrap();
    assert_eq!(folder.sidecar("a.jpg"), None);
    // Leaving with edits makes them Paste from Previous's.
    library.edit(with_contrast(&state, 0.5));
    let b = Item::photo(folder.photo("b.jpg"));
    library.open(&b);
    assert_eq!(library.previous_edits().unwrap().params.at("prep.contrast").as_f64(), Some(0.5));
    library.paste_from_previous(&[b.clone()]);
    assert_eq!(library.state().unwrap().params.at("prep.contrast").as_f64(), Some(0.5));
    assert_eq!(folder.raw("b.jpg").unwrap().at("variants.0.edits.params.prep.contrast").as_f64(), Some(0.5));
    assert_eq!(library.undo_name(), Some("Paste from Previous"));
    let outcome = library.undo().unwrap();
    assert!(outcome.open_changed);
    assert_eq!(folder.sidecar("b.jpg"), None);
    assert_eq!(library.state().unwrap().params.at("prep.contrast").as_f64(), Some(0.0));
}

#[test]
fn paste_takes_the_chosen_parts_and_reset_removes_them() {
    let folder = Folder::new("paste", &["a.jpg", "b.jpg", "c.jpg"]);
    let mut library = folder.library();
    let [a, b, c] = ["a.jpg", "b.jpg", "c.jpg"].map(|n| Item::photo(folder.photo(n)));
    library.open(&a);
    let mut state = with_contrast(library.state().unwrap(), 0.3);
    state.film = "kodak_portra_400".into();
    state.params.set_at("io.rotate_cw", Json::Number(1.0));
    library.edit(state);
    library.set_rating(&[c.clone()], 3);
    assert_eq!(library.copy_edits(&a, BTreeSet::from([EditGroup::FilmAndPaper])).note.unwrap().text, "Edits Copied");
    let c_before = folder.sidecar("c.jpg");
    let outcome = library.paste_edits(&[b.clone(), c.clone()]);
    assert_eq!(outcome.note.unwrap().text, "Edits Pasted");
    let b_edits = folder.raw("b.jpg").unwrap().at("variants.0.edits").clone();
    assert_eq!(b_edits.get("film").as_str(), Some("kodak_portra_400"));
    assert_eq!(b_edits.at("params.prep.contrast"), &Json::Null, "only the chosen part");
    assert_eq!(folder.raw("c.jpg").unwrap().at("variants.0.rating").as_integer(), Some(3), "the rating stays");
    assert_eq!(library.undo_name(), Some("Paste Edits"));
    library.undo();
    assert_eq!(folder.sidecar("b.jpg"), None);
    assert_eq!(folder.sidecar("c.jpg"), c_before);
    library.redo();
    assert!(library.summary(&b).edited && library.summary(&c).edited);
    library.reset_edits(&[c.clone(), a.clone()]);
    assert!(!library.summary(&c).edited);
    assert_eq!(library.summary(&c).rating, 3);
    assert_eq!(folder.raw("c.jpg").unwrap().at("variants.0.edits"), &Json::Null);
    assert!(!library.open_edited());
    assert_eq!(folder.sidecar("a.jpg"), None);
    assert_eq!(library.state().unwrap().params.at("io.rotate_cw").as_f64(), Some(0.0), "rotation goes too");
}

/// Copied from a photograph without edits, a paste takes its edits away
/// from those it lands on, in the parts chosen, and leaves the others.
#[test]
fn pasting_from_an_unedited_photograph() {
    let folder = Folder::new("unedited", &["a.jpg", "b.jpg", "c.jpg"]);
    let mut library = folder.library();
    let [a, b, c] = ["a.jpg", "b.jpg", "c.jpg"].map(|n| Item::photo(folder.photo(n)));
    library.open(&b);
    let mut state = library.state().unwrap().clone();
    state.film = "kodak_portra_400".into();
    library.edit(state);
    library.open(&c);
    let mut state = with_contrast(library.state().unwrap(), 0.2);
    state.params.set_at("io.rotate_cw", Json::Number(1.0));
    library.edit(state);
    library.open(&a);
    library.copy_edits(&a, EditGroup::standard());
    assert!(!library.copied().unwrap().edited);
    library.paste_edits(&[b.clone(), c.clone()]);
    assert_eq!(folder.sidecar("b.jpg"), None, "its only edit was the film");
    assert!(!library.summary(&b).edited);
    let c_edits = folder.raw("c.jpg").unwrap().at("variants.0.edits").clone();
    assert_eq!(c_edits.at("params.prep.contrast"), &Json::Null, "its Prep went");
    assert_eq!(c_edits.at("params.io.rotate_cw").as_f64(), Some(1.0), "its rotation, not pasted, stays");
}

#[test]
fn duplicates_are_named_and_removed_as_on_the_mac() {
    let folder = Folder::new("duplicates", &["IMG_1.CR3", "IMG_1.JPG", "b.jpg"]);
    let mut library = folder.library();
    let raw = Item::photo(folder.photo("IMG_1.CR3"));
    assert_eq!(library.items().len(), 2, "the camera JPEG goes with its RAW");
    assert_eq!(library.companion(&raw.photo), Some(folder.photo("IMG_1.JPG").as_path()));
    library.set_rating(&[raw.clone()], 2);
    let made = library.duplicate(&[raw.clone()]).select;
    assert_eq!(made.len(), 1);
    assert_eq!(library.name(&made[0]), "IMG_1.CR3 (2)");
    assert_eq!(library.summary(&made[0]).rating, 2, "with its rating");
    let again = library.duplicate(&[raw.clone()]).select;
    assert_eq!(library.name(&again[0]), "IMG_1.CR3 (3)");
    assert_eq!(library.undo_name(), Some("Duplicate"));
    assert_eq!(library.items().len(), 4);
    library.set_rating(&[made[0].clone()], 5);
    assert_eq!(library.summary(&raw).rating, 2, "a duplicate's own");

    let plan = library.delete_plan(&[made[0].clone()]);
    let question = plan.question(|i| library.name(i), library.delete_is_permanent(&plan), "Trash");
    assert_eq!(question.message, "Remove the duplicate “IMG_1.CR3 (2)”?");
    let before = folder.sidecar("IMG_1.CR3");
    library.delete(&plan);
    assert_eq!(library.name(&again[0]), "IMG_1.CR3 (2)");
    assert_eq!(library.undo_name(), Some("Remove Duplicate"));
    let outcome = library.undo().unwrap();
    assert_eq!(outcome.select, [made[0].clone()]);
    assert_eq!(folder.sidecar("IMG_1.CR3"), before);

    // The photograph goes with its sidecar, duplicates and camera JPEG.
    let plan = library.delete_plan(&[raw.clone()]);
    assert_eq!(plan.following, 2);
    let question = plan.question(|i| library.name(i), false, "Trash");
    assert_eq!(question.message, "Move “IMG_1.CR3” to the Trash?");
    assert!(question.information.contains("its camera JPEG") && question.information.contains("with its 2 duplicates"));
    library.delete(&plan);
    assert_eq!(library.items(), [Item::photo(folder.photo("b.jpg"))]);
    assert!(!folder.photo("IMG_1.JPG").exists() && !sidecar_path(&folder.photo("IMG_1.CR3")).exists());
    library.undo();
    assert_eq!(folder.sidecar("IMG_1.CR3"), before);
    assert!(folder.photo("IMG_1.JPG").exists());
    assert_eq!(library.items().len(), 4);
    library.redo();
    assert_eq!(library.items().len(), 1);
}

#[test]
fn another_apps_edits_replace_saved_ones_only() {
    let folder = Folder::new("outside", &["a.jpg"]);
    let mut library = folder.library();
    let a = Item::photo(folder.photo("a.jpg"));
    library.open(&a);
    library.edit(with_contrast(library.state().unwrap(), 0.1));
    library.save().unwrap();
    let mut raw = folder.raw("a.jpg").unwrap();
    raw.set_at("variants.0.edits.params.prep.contrast", Json::Number(0.7));
    raw.set_at("variants.0.edits.params.prep.shadows", Json::Number(0.2));
    // A different size, whatever the clock's resolution.
    sidecar::write_raw(&folder.photo("a.jpg"), Some(&raw)).unwrap();
    let (_, outcome) = library.refresh(false);
    assert!(outcome.open_changed);
    assert_eq!(library.state().unwrap().params.at("prep.contrast").as_f64(), Some(0.7));
    // With edits of its own unsaved, the open photograph keeps them, and
    // they are merged with the file when saved.
    library.edit(with_contrast(library.state().unwrap(), 0.4));
    raw.set_at("variants.0.rating", Json::from(3i64));
    sidecar::write_raw(&folder.photo("a.jpg"), Some(&raw)).unwrap();
    let (_, outcome) = library.refresh(false);
    assert!(!outcome.open_changed);
    library.save().unwrap();
    let raw = folder.raw("a.jpg").unwrap();
    assert_eq!(raw.at("variants.0.edits.params.prep.contrast").as_f64(), Some(0.4));
    assert_eq!(raw.at("variants.0.rating").as_integer(), Some(3));
}

#[test]
fn a_newer_or_unreadable_sidecar_is_never_written() {
    let folder = Folder::new("newer", &["a.jpg", "b.jpg"]);
    let newer = std::fs::read(root().join("docs/sidecar-fixtures/newer-schema.negs")).unwrap();
    std::fs::write(sidecar_path(&folder.photo("a.jpg")), &newer).unwrap();
    std::fs::write(sidecar_path(&folder.photo("b.jpg")), b"{ not json").unwrap();
    let mut library = folder.library();
    let a = Item::photo(folder.photo("a.jpg"));
    let b = Item::photo(folder.photo("b.jpg"));
    assert!(library.summary(&a).problem.unwrap().contains("newer version of Negs"));
    assert!(library.summary(&b).problem.is_some());
    let note = library.set_rating(&[a.clone(), b.clone()], 3).note.unwrap();
    assert_eq!(note.detail.as_deref(), Some("Not saved: its .negs file can’t be changed"));
    let opened = library.open(&a);
    assert!(opened.problem.is_some());
    library.edit(with_contrast(library.state().unwrap(), 0.5));
    library.save().unwrap();
    library.reset_edits(&[a.clone(), b.clone()]);
    library.duplicate(&[a.clone()]);
    assert_eq!(std::fs::read(sidecar_path(&folder.photo("a.jpg"))).unwrap(), newer);
    assert_eq!(std::fs::read(sidecar_path(&folder.photo("b.jpg"))).unwrap(), b"{ not json");
}

#[test]
fn a_new_photo_film_follows_into_an_unedited_open_photograph() {
    let folder = Folder::new("follow", &["a.jpg"]);
    let mut library = folder.library();
    library.open(&Item::photo(folder.photo("a.jpg")));
    let mut defaults = library.defaults().clone();
    defaults.new_photo_film = "kodak_portra_400".into();
    assert!(library.set_defaults(defaults).open_changed);
    assert_eq!(library.state().unwrap().film, "kodak_portra_400");
    library.save().unwrap();
    assert_eq!(folder.sidecar("a.jpg"), None, "not an edit");
}

#[test]
fn with_sidecars_off_nothing_is_written() {
    let folder = Folder::new("off", &["a.jpg"]);
    let mut library = folder.library();
    library.set_sidecars(false);
    let a = Item::photo(folder.photo("a.jpg"));
    let note = library.set_rating(&[a.clone()], 2).note.unwrap();
    assert!(note.detail.unwrap().contains("Settings"));
    assert_eq!(library.summary(&a).rating, 2, "kept for the session");
    library.open(&a);
    library.edit(with_contrast(library.state().unwrap(), 0.5));
    library.save().unwrap();
    assert_eq!(library.duplicate(&[a.clone()]).note.unwrap().text, "Not Duplicated");
    assert_eq!(folder.sidecar("a.jpg"), None);
}
