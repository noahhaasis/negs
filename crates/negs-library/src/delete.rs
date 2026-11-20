//! What a delete does, and the question before it (`DeletePlan` in
//! `Delete.swift`). A photograph goes to the bin with its sidecar, and a
//! RAW with its camera JPEG, only with all its duplicates. Otherwise what
//! is chosen leaves the sidecar and the file stays: a duplicate, or the
//! photograph itself, whose first duplicate left then takes its place.

use crate::item::Item;
use crate::sidecar;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

/// Where deleted files go. The platform's own bin (the Recycle Bin, the
/// Trash, the freedesktop trash); `FolderBin` for tests.
pub trait Bin {
    /// What the platform calls it, for the alert and the Undo menu
    /// ("Trash", "Recycle Bin").
    fn name(&self) -> &str {
        "Trash"
    }
    /// Whether `folder`'s volume has a bin. Without one, a delete is permanent.
    fn available(&self, folder: &Path) -> bool;
    /// Moves `file` to the bin and returns where it went, for Undo.
    fn trash(&mut self, file: &Path) -> io::Result<PathBuf>;
    /// Puts a file back from where `trash` put it.
    fn put_back(&mut self, trashed: &Path, original: &Path) -> io::Result<()>;
    /// Deletes `file` at once: on a volume without a bin.
    fn remove(&mut self, file: &Path) -> io::Result<()> {
        std::fs::remove_file(file)
    }
}

/// A bin that is a folder: files move into it under a unique name.
#[derive(Clone, Debug)]
pub struct FolderBin {
    pub folder: PathBuf,
    count: u64,
}

impl FolderBin {
    pub fn new(folder: impl Into<PathBuf>) -> FolderBin {
        FolderBin { folder: folder.into(), count: 0 }
    }
}

impl Bin for FolderBin {
    fn available(&self, _folder: &Path) -> bool {
        true
    }

    fn trash(&mut self, file: &Path) -> io::Result<PathBuf> {
        std::fs::create_dir_all(&self.folder)?;
        self.count += 1;
        let landed = self.folder.join(format!("{}-{}", self.count, crate::item::file_name(file)));
        std::fs::rename(file, &landed)?;
        Ok(landed)
    }

    fn put_back(&mut self, trashed: &Path, original: &Path) -> io::Result<()> {
        std::fs::rename(trashed, original)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeletePlan {
    /// Everything chosen, in order: what Undo selects again.
    pub items: Vec<Item>,
    /// Photographs whose files go.
    pub files: Vec<PathBuf>,
    /// What leaves the sidecar of a photograph that stays: duplicates, and
    /// photographs whose duplicates don't all go.
    pub duplicates: Vec<Item>,
    /// The duplicates that go with `files`.
    pub following: usize,
    /// The camera JPEGs that go with `files`.
    pub companions: HashMap<PathBuf, PathBuf>,
    /// Each duplicate that becomes its photograph, its photograph removed.
    pub renamed: Vec<(Item, Item)>,
}

/// The alert before a delete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    pub message: String,
    pub information: String,
    pub button: String,
}

impl DeletePlan {
    /// `duplicates` gives a photograph's duplicates by variant id,
    /// `companion` its camera JPEG.
    pub fn new(chosen: &[Item], duplicates: impl Fn(&Path) -> Vec<String>, companion: impl Fn(&Path) -> Option<PathBuf>) -> DeletePlan {
        let mut seen = HashSet::new();
        let chosen: Vec<Item> = chosen.iter().filter(|i| seen.insert((*i).clone())).cloned().collect();
        // A photograph's file goes only with every one of its duplicates.
        let files: Vec<PathBuf> = chosen
            .iter()
            .filter(|i| i.variant.is_none() && duplicates(&i.photo).iter().all(|id| seen.contains(&Item::duplicate(&i.photo, id.as_str()))))
            .map(|i| i.photo.clone())
            .collect();
        let trashed: HashSet<&PathBuf> = files.iter().collect();
        let removed: Vec<Item> = chosen.iter().filter(|i| !trashed.contains(&i.photo)).cloned().collect();
        let items = chosen.iter().filter(|i| i.variant.is_none() || !trashed.contains(&i.photo)).cloned().collect();
        let following = files.iter().map(|file| duplicates(file).len()).sum();
        let companions = files.iter().filter_map(|file| companion(file).map(|p| (file.clone(), p))).collect();
        let renamed = removed
            .iter()
            .filter(|i| i.variant.is_none())
            .filter_map(|photo| {
                let first = duplicates(&photo.photo).into_iter().find(|id| !seen.contains(&Item::duplicate(&photo.photo, id.as_str())))?;
                Some((Item::duplicate(&photo.photo, first), photo.clone()))
            })
            .collect();
        DeletePlan { items, files, duplicates: removed, following, companions, renamed }
    }

    /// What `item` is called once the plan is done: a duplicate that takes
    /// its photograph's place becomes the photograph.
    pub fn after(&self, item: &Item) -> Item {
        self.renamed.iter().find(|(from, _)| from == item).map(|(_, to)| to.clone()).unwrap_or_else(|| item.clone())
    }

    /// The photographs among `duplicates`, kept for their duplicates.
    fn kept_photographs(&self) -> usize {
        self.duplicates.iter().filter(|i| i.variant.is_none()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The same plan without its files: after a permanent delete, only the
    /// duplicates can come back and go again.
    pub fn without_files(&self) -> DeletePlan {
        DeletePlan {
            items: self.duplicates.clone(),
            files: Vec::new(),
            duplicates: self.duplicates.clone(),
            following: 0,
            companions: HashMap::new(),
            renamed: self.renamed.clone(),
        }
    }

    /// The Undo menu's name for it. `bin` is what the platform calls it
    /// ("Trash", "Recycle Bin").
    pub fn action_name(&self, bin: &str) -> String {
        if !self.files.is_empty() {
            format!("Move to {bin}")
        } else if self.kept_photographs() > 0 {
            if self.duplicates.len() > 1 { "Remove Photographs" } else { "Remove Photograph" }.into()
        } else if self.duplicates.len() > 1 {
            "Remove Duplicates".into()
        } else {
            "Remove Duplicate".into()
        }
    }

    /// The files that go for a photograph: itself, its sidecar, and its
    /// camera JPEG with that one's sidecar, those that exist.
    pub fn files_of(&self, photo: &Path) -> Vec<PathBuf> {
        let mut out = vec![photo.to_path_buf()];
        let sidecar = sidecar::sidecar_path(photo);
        if sidecar.exists() {
            out.push(sidecar);
        }
        if let Some(picture) = self.companions.get(photo) {
            for file in [picture.clone(), sidecar::sidecar_path(picture)] {
                if file.exists() {
                    out.push(file);
                }
            }
        }
        out
    }

    /// The alert before it, as macOS words it. A single photograph is named
    /// rather than counted. `name` gives an item's name in the strip.
    /// `bin` is what the platform calls it ("Trash", "Recycle Bin").
    pub fn question(&self, name: impl Fn(&Item) -> String, permanent: bool, bin: &str) -> Question {
        let photos = self.files.len();
        let copies = self.duplicates.len();
        let following = self.following;
        let count = |n: usize, noun: &str| if n == 1 { format!("1 {noun}") } else { format!("{n} {noun}s") };
        let with = if following == 0 {
            String::new()
        } else if photos == 1 {
            if following == 1 { ", with its duplicate".into() } else { format!(", with its {following} duplicates") }
        } else {
            format!(", with {}", count(following, "duplicate"))
        };
        let files_go = match (self.companions.is_empty(), photos == 1) {
            (true, true) => "the photograph’s file and its Negs rating and edits",
            (true, false) => "the photograph files and their Negs ratings and edits",
            (false, true) => "the photograph’s file, its camera JPEG and its Negs rating and edits",
            (false, false) => "the photograph files, their camera JPEGs and their Negs ratings and edits",
        };
        let kept = self.kept_photographs();
        let copies_go = match (kept, copies) {
            (0, 1) => "The duplicate’s rating and edits are removed from its .negs file; the photograph stays.".to_string(),
            (0, _) => "The duplicates’ ratings and edits are removed from their .negs files; the photographs stay.".to_string(),
            (_, 1) => match self.renamed.first() {
                Some((next, _)) => format!("Its rating and edits are removed from its .negs file; the file stays, and its duplicate “{}” takes its place.", name(next)),
                None => "Its rating and edits are removed from its .negs file; the file stays for its duplicates.".to_string(),
            },
            _ => "Their ratings and edits are removed from their .negs files; the files stay, and the first duplicate left takes each photograph’s place.".to_string(),
        };
        // What leaves the sidecars, counted: "1 photograph and 2 duplicates".
        let removals = match (kept, copies - kept) {
            (0, d) => count(d, "duplicate"),
            (k, 0) => count(k, "photograph"),
            (k, d) => format!("{} and {}", count(k, "photograph"), count(d, "duplicate")),
        };
        if photos == 0 {
            let message = match (copies, kept) {
                (1, 0) => format!("Remove the duplicate “{}”?", name(&self.duplicates[0])),
                (1, _) => format!("Remove “{}”?", name(&self.duplicates[0])),
                (_, 0) => format!("Remove {copies} duplicates?"),
                _ => format!("Remove {removals}?"),
            };
            return Question {
                message,
                information: format!("{copies_go} You can put {} back with Undo.", if copies == 1 { "it" } else { "them" }),
                button: "Remove".into(),
            };
        }
        let what = if photos == 1 && copies == 0 { format!("“{}”", name(&Item::photo(&self.files[0]))) } else { count(photos, "photograph") };
        let and_copies = if copies == 0 { String::new() } else { format!(" and remove {removals}") };
        if permanent {
            let back = if copies == 0 { "" } else { " The duplicates can be put back with Undo." };
            let also = if copies == 0 { String::new() } else { format!(" {copies_go}") };
            return Question {
                message: format!("Delete {what} permanently{and_copies}?"),
                information: format!("This volume has no {bin}, so {files_go} are deleted at once{with}. This can’t be undone.{also}{back}"),
                button: "Delete".into(),
            };
        }
        let back = if photos == 1 && copies == 0 {
            format!("You can put it back with Undo or from the {bin}.")
        } else {
            format!("You can put them back with Undo or from the {bin}.")
        };
        let mut first = files_go.to_string();
        if let Some(c) = first.get(..1) {
            first = c.to_uppercase() + &first[1..];
        }
        let also = if copies == 0 { String::new() } else { format!("{copies_go} ") };
        Question {
            message: format!("Move {what} to the {bin}{and_copies}?"),
            information: format!("{first} are moved to the {bin}{with}. {also}{back}"),
            button: if copies == 0 { format!("Move to {bin}") } else { "Delete".into() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_and_their_questions() {
        let a = Item::photo("/p/a.CR3");
        let a2 = Item::duplicate("/p/a.CR3", "d1");
        let a3 = Item::duplicate("/p/a.CR3", "d3");
        let b2 = Item::duplicate("/p/b.CR3", "d2");
        let ids = |p: &Path| if p == Path::new("/p/a.CR3") { vec!["d1".to_string(), "d3".to_string()] } else { vec!["d2".to_string()] };
        let jpeg = |p: &Path| (p == Path::new("/p/a.CR3")).then(|| PathBuf::from("/p/a.JPG"));
        let plan = DeletePlan::new(&[a.clone(), a2.clone(), a3.clone(), b2.clone(), a.clone()], ids, jpeg);
        assert_eq!(plan.items, [a.clone(), b2.clone()], "a duplicate of a photograph that goes goes with it");
        assert_eq!(plan.files, [PathBuf::from("/p/a.CR3")], "with all its duplicates, the file goes");
        assert_eq!(plan.duplicates, [b2.clone()]);
        assert_eq!(plan.following, 2);
        assert!(plan.renamed.is_empty());
        assert_eq!(plan.action_name("Recycle Bin"), "Move to Recycle Bin");
        let name = |i: &Item| match &i.variant {
            None => i.file_name(),
            Some(id) => format!("{} ({id})", i.file_name()),
        };
        let q = plan.question(name, false, "Recycle Bin");
        assert_eq!(q.message, "Move 1 photograph to the Recycle Bin and remove 1 duplicate?");
        assert_eq!(
            q.information,
            "The photograph’s file, its camera JPEG and its Negs rating and edits are moved to the Recycle Bin, with its 2 duplicates. The duplicate’s rating and edits are removed from its .negs file; the photograph stays. You can put them back with Undo or from the Recycle Bin."
        );
        assert_eq!(q.button, "Delete");

        // A photograph whose duplicates stay leaves its sidecar; the first left takes its place.
        let kept = DeletePlan::new(&[a.clone()], ids, jpeg);
        assert!(kept.files.is_empty() && kept.companions.is_empty());
        assert_eq!(kept.duplicates, [a.clone()]);
        assert_eq!(kept.renamed, [(a2.clone(), a.clone())]);
        assert_eq!(kept.after(&a2), a);
        assert_eq!(kept.after(&a3), a3);
        assert_eq!(kept.action_name("Trash"), "Remove Photograph");
        let q = kept.question(name, false, "Trash");
        assert_eq!(q.message, "Remove “a.CR3”?");
        assert_eq!(
            q.information,
            "Its rating and edits are removed from its .negs file; the file stays, and its duplicate “a.CR3 (d1)” takes its place. You can put it back with Undo."
        );
        assert_eq!(q.button, "Remove");
        assert!(!DeletePlan::new(&[a.clone()], ids, jpeg).question(name, true, "Trash").message.contains("permanently"));
        let some = DeletePlan::new(&[a.clone(), a2.clone()], ids, jpeg);
        assert_eq!(some.renamed, [(a3.clone(), a.clone())]);
        assert_eq!(some.question(name, false, "Trash").message, "Remove 1 photograph and 1 duplicate?");
        assert_eq!(some.action_name("Trash"), "Remove Photographs");
        let one = DeletePlan::new(&[Item::photo("/p/c.jpg")], |_| vec![], |_| None);
        let q = one.question(name, false, "Trash");
        assert_eq!(q.message, "Move “c.jpg” to the Trash?");
        assert_eq!(q.information, "The photograph’s file and its Negs rating and edits are moved to the Trash. You can put it back with Undo or from the Trash.");
        assert_eq!(q.button, "Move to Trash");
        let q = one.question(name, true, "Trash");
        assert_eq!(q.message, "Delete “c.jpg” permanently?");
        assert_eq!(q.information, "This volume has no Trash, so the photograph’s file and its Negs rating and edits are deleted at once. This can’t be undone.");
        let copies = DeletePlan::new(&[b2.clone()], ids, jpeg);
        assert_eq!(copies.action_name("Trash"), "Remove Duplicate");
        let q = copies.question(name, false, "Trash");
        assert_eq!(q.message, "Remove the duplicate “b.CR3 (d2)”?");
        assert_eq!(q.button, "Remove");
        assert_eq!(plan.without_files().items, [b2]);
    }
}
