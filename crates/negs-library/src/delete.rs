//! What a delete does, and the question before it (`DeletePlan` in
//! `Delete.swift`). Photographs go to the bin with their sidecars, and so
//! with all their duplicates, and a RAW with its camera JPEG. A duplicate
//! of a photograph that stays is removed from the sidecar; the file stays.

use crate::item::Item;
use crate::sidecar;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

/// Where deleted files go. The platform's own bin (the Recycle Bin, the
/// Trash, the freedesktop trash); `FolderBin` for tests.
pub trait Bin {
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
    /// Duplicates removed from the sidecar of a photograph that stays.
    pub duplicates: Vec<Item>,
    /// Duplicates that go with their photographs without being chosen.
    pub following: usize,
    /// The camera JPEGs that go with `files`.
    pub companions: HashMap<PathBuf, PathBuf>,
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
        let files: Vec<PathBuf> = chosen.iter().filter(|i| i.variant.is_none()).map(|i| i.photo.clone()).collect();
        let trashed: HashSet<&PathBuf> = files.iter().collect();
        let removed = chosen.iter().filter(|i| i.variant.is_some() && !trashed.contains(&i.photo)).cloned().collect();
        let items = chosen.iter().filter(|i| i.variant.is_none() || !trashed.contains(&i.photo)).cloned().collect();
        let following = files.iter().map(|file| duplicates(file).iter().filter(|id| !seen.contains(&Item::duplicate(file, id.as_str()))).count()).sum();
        let companions = files.iter().filter_map(|file| companion(file).map(|p| (file.clone(), p))).collect();
        DeletePlan { items, files, duplicates: removed, following, companions }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The same plan without its files: after a permanent delete, only the
    /// duplicates can come back and go again.
    pub fn without_files(&self) -> DeletePlan {
        DeletePlan { items: self.duplicates.clone(), files: Vec::new(), duplicates: self.duplicates.clone(), following: 0, companions: HashMap::new() }
    }

    /// The Undo menu's name for it. `bin` is what the platform calls it
    /// ("Trash", "Recycle Bin").
    pub fn action_name(&self, bin: &str) -> String {
        if !self.files.is_empty() {
            format!("Move to {bin}")
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
        let copies_go = if copies == 1 {
            "The duplicate’s rating and edits are removed from its .negs file; the photograph stays."
        } else {
            "The duplicates’ ratings and edits are removed from their .negs files; the photographs stay."
        };
        if photos == 0 {
            let message = if copies == 1 { format!("Remove the duplicate “{}”?", name(&self.duplicates[0])) } else { format!("Remove {copies} duplicates?") };
            return Question {
                message,
                information: format!("{copies_go} You can put {} back with Undo.", if copies == 1 { "it" } else { "them" }),
                button: "Remove".into(),
            };
        }
        let what = if photos == 1 && copies == 0 { format!("“{}”", name(&Item::photo(&self.files[0]))) } else { count(photos, "photograph") };
        let and_copies = if copies == 0 { String::new() } else { format!(" and remove {}", count(copies, "duplicate")) };
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
        let b2 = Item::duplicate("/p/b.CR3", "d2");
        let ids = |p: &Path| if p == Path::new("/p/a.CR3") { vec!["d1".to_string(), "d3".to_string()] } else { vec!["d2".to_string()] };
        let jpeg = |p: &Path| (p == Path::new("/p/a.CR3")).then(|| PathBuf::from("/p/a.JPG"));
        let plan = DeletePlan::new(&[a.clone(), a2.clone(), b2.clone(), a.clone()], ids, jpeg);
        assert_eq!(plan.items, [a.clone(), b2.clone()], "a duplicate of a photograph that goes goes with it");
        assert_eq!(plan.files, [PathBuf::from("/p/a.CR3")]);
        assert_eq!(plan.duplicates, [b2.clone()]);
        assert_eq!(plan.following, 1, "d3 follows unchosen");
        assert_eq!(plan.action_name("Recycle Bin"), "Move to Recycle Bin");
        let name = |i: &Item| match &i.variant {
            None => i.file_name(),
            Some(_) => format!("{} (2)", i.file_name()),
        };
        let q = plan.question(name, false, "Recycle Bin");
        assert_eq!(q.message, "Move 1 photograph to the Recycle Bin and remove 1 duplicate?");
        assert_eq!(
            q.information,
            "The photograph’s file, its camera JPEG and its Negs rating and edits are moved to the Recycle Bin, with its duplicate. The duplicate’s rating and edits are removed from its .negs file; the photograph stays. You can put them back with Undo or from the Recycle Bin."
        );
        assert_eq!(q.button, "Delete");
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
        assert_eq!(q.message, "Remove the duplicate “b.CR3 (2)”?");
        assert_eq!(q.button, "Remove");
        assert_eq!(plan.without_files().items, [b2]);
    }
}
