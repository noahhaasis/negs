//! What the film strip and grid show: photographs and their duplicates
//! (`Library.swift`: the `#variant=` URLs, `LibrarySummary`).

use crate::sidecar::{Flag, Sidecar, Variant};
use std::path::{Path, PathBuf};

/// One thing the strip shows: a photograph (`variant: None`, its sidecar's
/// first variant) or one of its duplicates (a later variant, by id).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Item {
    pub photo: PathBuf,
    pub variant: Option<String>,
}

impl Item {
    pub fn photo(photo: impl Into<PathBuf>) -> Item {
        Item { photo: photo.into(), variant: None }
    }

    pub fn duplicate(photo: impl Into<PathBuf>, id: impl Into<String>) -> Item {
        Item { photo: photo.into(), variant: Some(id.into()) }
    }

    pub fn is_duplicate(&self) -> bool {
        self.variant.is_some()
    }

    /// The photograph's own file name.
    pub fn file_name(&self) -> String {
        file_name(&self.photo)
    }
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// What the strip shows of an item: its rating and flag, and whether it
/// has edits (`LibrarySummary`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub rating: u8,
    pub flag: Option<Flag>,
    pub edited: bool,
    /// Why its sidecar isn't used (unreadable, from a newer Negs): the
    /// message to show.
    pub problem: Option<String>,
}

impl Summary {
    pub fn of(variant: &Variant) -> Summary {
        Summary { rating: variant.rating, flag: variant.flag, edited: variant.edits.is_some(), problem: None }
    }
}

/// Each library item of `photo` as its sidecar holds them: itself (the
/// first variant) and its duplicates, with their summaries and looks.
pub fn items_of(photo: &Path, sidecar: Option<&Sidecar>) -> Vec<(Item, Summary, Option<String>)> {
    let Some(sidecar) = sidecar.filter(|s| !s.variants.is_empty()) else { return vec![(Item::photo(photo), Summary::default(), None)] };
    sidecar
        .variants
        .iter()
        .enumerate()
        .map(|(index, variant)| {
            let item = if index == 0 { Item::photo(photo) } else { Item::duplicate(photo, &variant.id) };
            let look = variant.edits.as_ref().map(|edits| crate::thumbnail::look(edits, sidecar.defaults_version));
            (item, Summary::of(variant), look)
        })
        .collect()
}

/// "No Rating", or the stars (`EditorModel.ratingText`).
pub fn rating_text(stars: u8) -> String {
    if stars == 0 { "No Rating".into() } else { "★".repeat(stars as usize) }
}

/// "Pick", "Rejected" or "Unflagged" (`EditorModel.flagText`).
pub fn flag_text(flag: Option<Flag>) -> &'static str {
    match flag {
        Some(Flag::Pick) => "Pick",
        Some(Flag::Reject) => "Rejected",
        None => "Unflagged",
    }
}
