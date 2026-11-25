//! Compare (ticket 46, `Compare.swift`): two photographs side by side or
//! in one frame with a line between them, or a photograph against its
//! original; and what Original ⌄ makes the Original word do. The rules and
//! geometry, with no Win32, so `windows/library-map` tests them on any OS.

use negs_library::Item;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    SideBySide,
    Slider,
}

/// Where compare was entered, which leaving goes back to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Grid,
    Photo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Side {
    Photo(Item),
    /// The photograph's original, undeveloped.
    Original(Item),
}

impl Side {
    pub fn item(&self) -> &Item {
        match self {
            Side::Photo(item) | Side::Original(item) => item,
        }
    }

    pub fn is_original(&self) -> bool {
        matches!(self, Side::Original(_))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// Left or top, and right or bottom.
    pub first: Side,
    pub second: Side,
    /// The first is the active one: the one the panel edits, or against the
    /// original, the one last clicked.
    pub active_first: bool,
    pub layout: Layout,
    pub origin: Origin,
    /// The slider's line, as a fraction of the frame's width.
    pub split: f64,
}

impl Comparison {
    pub fn new(first: Side, second: Side, active_first: bool, origin: Origin) -> Self {
        Self { first, second, active_first, layout: Layout::SideBySide, origin, split: 0.5 }
    }

    pub fn active(&self) -> &Side {
        if self.active_first { &self.first } else { &self.second }
    }

    pub fn inactive(&self) -> &Side {
        if self.active_first { &self.second } else { &self.first }
    }

    pub fn against_original(&self) -> bool {
        self.first.is_original() || self.second.is_original()
    }

    /// The photograph the panel edits: the active one, or the developed one
    /// beside its original.
    pub fn edited(&self) -> &Item {
        if self.active().is_original() { self.inactive().item() } else { self.active().item() }
    }
}

/// How the context menu's item and the Compare word read for some photographs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub title: String,
    pub enabled: bool,
    /// The two to compare, first then second; None with the original.
    pub pair: Option<(Item, Item)>,
    pub with_original: bool,
}

/// The item for a right-click on `item` acting on `targets` (the selection
/// it's in, or itself): two compare with each other, three or more can't,
/// one compares with the current photograph, or with its original.
pub fn offer(targets: &[Item], current: Option<&Item>, item: &Item, number: &dyn Fn(&Item) -> String, can_compare: bool, can_with_original: bool, original: &str) -> Offer {
    if targets.len() > 2 {
        return Offer { title: "Compare Two Photographs".into(), enabled: false, pair: None, with_original: false };
    }
    if let [first, second] = targets {
        return Offer { title: format!("Compare {} and {}", number(first), number(second)), enabled: can_compare, pair: Some((first.clone(), second.clone())), with_original: false };
    }
    if let Some(current) = current.filter(|c| *c != item) {
        return Offer { title: format!("Compare with {}", number(current)), enabled: can_compare, pair: Some((current.clone(), item.clone())), with_original: false };
    }
    Offer { title: format!("Compare with {original}"), enabled: can_compare && can_with_original, pair: None, with_original: true }
}

/// The title bar's Compare word, the C key and View › Compare: whether
/// they act on the selection (in shown order), and the word's tooltip.
pub fn word(selected: &[Item], number: &dyn Fn(&Item) -> String, can_compare: bool) -> (bool, String) {
    match selected {
        [first, second] if can_compare => (true, format!("Compare {} and {} (C)", number(first), number(second))),
        _ if selected.len() > 2 => (false, "Compare takes two photographs. Three or more can't be compared.".into()),
        _ => (false, "Select a second photograph to compare (Ctrl-click)".into()),
    }
}

/// The View menu's Compare: "Stop Comparing" while comparing.
/// The camera's facts by part (`PhotoFacts.parts`), as the status line
/// names them.
pub const PARTS: [&str; 5] = ["camera", "ISO", "shutter", "aperture", "focal length"];

/// What two photographs differ in, as the status line says it after
/// "Comparing 3 and 7 · differ in": "shutter", "film and paper", "ISO,
/// shutter and film" (`CompareDifferences`). The facts (`PhotoFacts::parts`)
/// count where either has them; nothing while either's are unknown.
pub fn differences(a: Option<&[Option<String>; 5]>, b: Option<&[Option<String>; 5]>, film: (&str, &str), paper: (Option<&str>, Option<&str>)) -> String {
    let mut names: Vec<&str> = vec![];
    if let (Some(a), Some(b)) = (a, b) {
        for (i, name) in PARTS.into_iter().enumerate() {
            if (a[i].is_some() || b[i].is_some()) && a[i] != b[i] {
                names.push(name);
            }
        }
    }
    if film.0 != film.1 {
        names.push("film");
    }
    if paper.0 != paper.1 {
        names.push("paper");
    }
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => last.to_string(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

pub fn menu_title(comparing: bool) -> &'static str {
    if comparing { "Stop C&omparing\tC" } else { "C&ompare\tC" }
}

/// ← →: the photograph `offset` places from `from` in `order`, skipping
/// `first`; None past the ends. One no longer shown steps from the first.
pub fn step(from: &Item, first: &Item, order: &[Item], offset: isize) -> Option<Item> {
    let candidates: Vec<&Item> = order.iter().filter(|i| *i != first).collect();
    let start = if let Some(index) = candidates.iter().position(|i| *i == from) {
        index as isize
    } else if let Some(index) = order.iter().position(|i| i == first) {
        // From where the first is: the one after it is a step on.
        if offset > 0 { index as isize - 1 } else { index as isize }
    } else {
        return None;
    };
    let to = start + offset;
    (0..candidates.len() as isize).contains(&to).then(|| candidates[to as usize].clone())
}

/// The number a photograph is known by on the roll: its place in the
/// folder, counted from 1, and a duplicate's letter after it (7a, 7b).
/// `items` is the folder's, every duplicate in order.
pub fn frame_number(items: &[Item], item: &Item) -> String {
    let mut photos: Vec<&std::path::Path> = vec![];
    for i in items {
        if photos.last() != Some(&i.photo.as_path()) && !photos.contains(&i.photo.as_path()) {
            photos.push(&i.photo);
        }
    }
    let Some(index) = photos.iter().position(|p| *p == item.photo.as_path()) else { return String::new() };
    let siblings: Vec<&Item> = items.iter().filter(|i| i.photo == item.photo).collect();
    if item.variant.is_none() || siblings.len() < 2 {
        return format!("{}", index + 1);
    }
    let duplicate = siblings.iter().position(|i| *i == item).unwrap_or(0);
    let letters = "abcdefghijklmnopqrstuvwxyz".as_bytes();
    match letters.get(duplicate) {
        Some(letter) => format!("{}{}", index + 1, *letter as char),
        None => format!("{}·{}", index + 1, duplicate + 2),
    }
}

/// What Original does in the photo view, remembered (`OriginalMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OriginalMode {
    /// Hold it to see the whole original.
    #[default]
    Hold,
    /// A click shows the original left of a line that can be dragged.
    Split,
    /// A click compares the photograph with its original.
    SideBySide,
}

impl OriginalMode {
    pub const ALL: [OriginalMode; 3] = [OriginalMode::Hold, OriginalMode::Split, OriginalMode::SideBySide];

    pub fn key(self) -> &'static str {
        match self {
            OriginalMode::Hold => "hold",
            OriginalMode::Split => "split",
            OriginalMode::SideBySide => "sideBySide",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL.into_iter().find(|m| m.key() == key).unwrap_or_default()
    }

    pub fn title(self) -> &'static str {
        match self {
            OriginalMode::Hold => "Hold to see the whole",
            OriginalMode::Split => "Split line",
            OriginalMode::SideBySide => "Side by side",
        }
    }

    /// The Original word's tooltip; `original` is "original" or "camera JPEG".
    #[allow(dead_code)]
    pub fn help(self, original: &str) -> String {
        match self {
            OriginalMode::Hold => format!("Hold to show the {original} (\\ or O). Hold T to turn the photograph upside down."),
            OriginalMode::Split => format!("Show the {original} left of a line you can drag. Hold \\ to see the whole {original}."),
            OriginalMode::SideBySide => format!("Compare with the {original}, side by side. Hold \\ to see the whole {original}."),
        }
    }
}

/// What Original shows of a RAW shot as RAW+JPEG, remembered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OriginalReference {
    #[default]
    Raw,
    CameraJpeg,
}

impl OriginalReference {
    pub const ALL: [OriginalReference; 2] = [OriginalReference::Raw, OriginalReference::CameraJpeg];

    pub fn key(self) -> &'static str {
        match self {
            OriginalReference::Raw => "raw",
            OriginalReference::CameraJpeg => "cameraJPEG",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL.into_iter().find(|r| r.key() == key).unwrap_or_default()
    }

    pub fn title(self) -> &'static str {
        match self {
            OriginalReference::Raw => "Show the RAW",
            OriginalReference::CameraJpeg => "Show the camera JPEG",
        }
    }
}

// MARK: Geometry (`CompareArrangement`), in DIPs.

/// Above each photograph in a row: its caption; below: its label.
pub const CAPTION: f64 = 54.0;
pub const LABEL: f64 = 68.0;
/// Stacked, only a caption line.
pub const STACKED_CAPTION: f64 = 40.0;
/// Between the two, and round the canvas's edges.
pub const GAP: f64 = 28.0;
pub const STACKED_GAP: f64 = 14.0;
pub const SIDE: f64 = 36.0;
pub const BOTTOM: f64 = 12.0;

/// x, y, width, height.
pub type Frame = [f64; 4];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrangement {
    Row,
    Column,
}

pub fn fit(aspect: f64, room: (f64, f64)) -> (f64, f64) {
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.5 };
    if room.0 / room.1 > aspect { (room.1 * aspect, room.1) } else { (room.0, room.0 / aspect) }
}

pub fn row_layout(canvas: (f64, f64), aspects: (f64, f64)) -> [Frame; 2] {
    let width = ((canvas.0 - 2.0 * SIDE - GAP) / 2.0).max(1.0);
    let height = (canvas.1 - CAPTION - LABEL).max(1.0);
    let place = |index: usize, aspect: f64| {
        let size = fit(aspect, (width, height));
        let x = SIDE + index as f64 * (width + GAP) + (width - size.0) / 2.0;
        [x.round(), (CAPTION + (height - size.1) / 2.0).round(), size.0.round(), size.1.round()]
    };
    [place(0, aspects.0), place(1, aspects.1)]
}

pub fn column_layout(canvas: (f64, f64), aspects: (f64, f64)) -> [Frame; 2] {
    let width = (canvas.0 - 2.0 * SIDE).max(1.0);
    let height = ((canvas.1 - 2.0 * STACKED_CAPTION - STACKED_GAP - BOTTOM) / 2.0).max(1.0);
    let sizes = [fit(aspects.0, (width, height)), fit(aspects.1, (width, height))];
    // The pair sits in the middle of the canvas, as one block.
    let block = 2.0 * STACKED_CAPTION + STACKED_GAP + sizes[0].1 + sizes[1].1;
    let mut y = ((canvas.1 - BOTTOM - block) / 2.0).max(0.0);
    let mut frames = [[0.0; 4]; 2];
    for (i, size) in sizes.iter().enumerate() {
        y += STACKED_CAPTION;
        frames[i] = [((canvas.0 - size.0) / 2.0).round(), y.round(), size.0.round(), size.1.round()];
        y += size.1 + STACKED_GAP;
    }
    frames
}

/// The arrangement that shows the two larger: in a row, or one above the other.
pub fn layout(canvas: (f64, f64), aspects: (f64, f64)) -> (Arrangement, [Frame; 2]) {
    let area = |f: &[Frame; 2]| f.iter().map(|r| r[2] * r[3]).sum::<f64>();
    let (row, column) = (row_layout(canvas, aspects), column_layout(canvas, aspects));
    if area(&column) > area(&row) { (Arrangement::Column, column) } else { (Arrangement::Row, row) }
}

/// The slider's one frame, at the first photograph's proportions.
pub fn slider_frame(canvas: (f64, f64), aspect: f64) -> Frame {
    let room = ((canvas.0 - 2.0 * SIDE).max(1.0), (canvas.1 - CAPTION - LABEL).max(1.0));
    let size = fit(aspect, room);
    [((canvas.0 - size.0) / 2.0).round(), (CAPTION + (room.1 - size.1) / 2.0).round(), size.0.round(), size.1.round()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn item(name: &str) -> Item {
        Item::photo(Path::new(name))
    }

    fn number(i: &Item) -> String {
        i.photo.to_string_lossy().trim_start_matches('p').to_string()
    }

    #[test]
    fn the_status_line_says_what_differs() {
        let facts = |iso: &str, shutter: &str, aperture: &str, focal: Option<&str>| {
            [Some("Nikon D750".to_string()), Some(iso.to_string()), Some(shutter.to_string()), Some(aperture.to_string()), focal.map(String::from)]
        };
        let a = facts("ISO 400", "1/160", "f/4", None);
        let b = facts("ISO 400", "1/250", "f/5.6", None);
        assert_eq!(differences(Some(&a), Some(&b), ("Portra", "Portra"), (Some("Endura"), Some("Endura"))), "shutter and aperture");
        assert_eq!(differences(Some(&a), Some(&a), ("Portra", "Gold"), (Some("Endura"), None)), "film and paper");
        let c = facts("ISO 800", "1/250", "f/5.6", None);
        assert_eq!(differences(Some(&a), Some(&c), ("Portra", "Gold"), (None, None)), "ISO, shutter, aperture and film");
        // A part only one has differs; unknown facts don't, yet.
        let d = facts("ISO 400", "1/160", "f/4", Some("50 mm"));
        assert_eq!(differences(Some(&a), Some(&d), ("P", "P"), (None, None)), "focal length");
        assert_eq!(differences(Some(&a), None, ("P", "P"), (None, None)), "");
    }

    #[test]
    fn compare_is_offered_for_two_never_three() {
        let (a, b, c) = (item("p3"), item("p7"), item("p9"));
        let three = offer(&[a.clone(), b.clone(), c.clone()], Some(&a), &a, &number, true, true, "Original");
        assert_eq!((three.title.as_str(), three.enabled, three.pair.is_none()), ("Compare Two Photographs", false, true));
        let two = offer(&[a.clone(), b.clone()], Some(&a), &a, &number, true, true, "Original");
        assert_eq!((two.title.as_str(), two.enabled), ("Compare 3 and 7", true));
        assert_eq!(two.pair, Some((a.clone(), b.clone())));
        assert!(!offer(&[a.clone(), b.clone()], None, &a, &number, false, true, "Original").enabled, "while cropping, say");
        // One: with the current photograph, or itself with its original.
        let with = offer(&[b.clone()], Some(&a), &b, &number, true, true, "Original");
        assert_eq!((with.title.as_str(), with.pair), ("Compare with 3", Some((a.clone(), b.clone()))));
        let original = offer(&[a.clone()], Some(&a), &a, &number, true, true, "Camera JPEG");
        assert_eq!((original.title.as_str(), original.enabled, original.with_original), ("Compare with Camera JPEG", true, true));
        assert!(!offer(&[a.clone()], Some(&a), &a, &number, true, false, "Original").enabled, "in the grid, say");
    }

    #[test]
    fn the_compare_word_acts_on_two_selected() {
        let (a, b, c) = (item("p3"), item("p7"), item("p9"));
        assert_eq!(word(&[a.clone(), b.clone()], &number, true), (true, "Compare 3 and 7 (C)".into()));
        assert!(!word(&[a.clone(), b.clone()], &number, false).0);
        assert_eq!(word(&[a.clone()], &number, true), (false, "Select a second photograph to compare (Ctrl-click)".into()));
        assert_eq!(word(&[], &number, true).0, false);
        assert_eq!(word(&[a, b, c], &number, true), (false, "Compare takes two photographs. Three or more can't be compared.".into()));
        assert_eq!((menu_title(false), menu_title(true)), ("C&ompare\tC", "Stop C&omparing\tC"));
    }

    #[test]
    fn steps_skip_the_first_and_stop_at_the_ends() {
        let order: Vec<Item> = ["p1", "p2", "p3", "p4"].iter().map(|n| item(n)).collect();
        assert_eq!(step(&order[1], &order[2], &order, 1), Some(order[3].clone()), "past the first");
        assert_eq!(step(&order[3], &order[0], &order, 1), None, "the end stops it");
        assert_eq!(step(&order[1], &order[0], &order, -1), None);
        // The second is gone from view: steps from where the first is.
        assert_eq!(step(&item("p9"), &order[1], &order, 1), Some(order[2].clone()));
        assert_eq!(step(&item("p9"), &order[1], &order, -1), Some(order[0].clone()));
    }

    #[test]
    fn frame_numbers_count_photographs_and_letter_duplicates() {
        let dup = |name: &str, v: &str| Item { photo: Path::new(name).into(), variant: Some(v.into()) };
        let items = vec![item("a"), dup("b", "x"), dup("b", "y"), item("c")];
        assert_eq!(frame_number(&items, &items[0]), "1");
        assert_eq!(frame_number(&items, &items[1]), "2a");
        assert_eq!(frame_number(&items, &items[2]), "2b");
        assert_eq!(frame_number(&items, &items[3]), "3");
        assert_eq!(frame_number(&items, &item("z")), "");
    }

    #[test]
    fn two_landscapes_stack_in_a_tall_window() {
        let (arrangement, frames) = layout((900.0, 1200.0), (1.5, 1.5));
        assert_eq!(arrangement, Arrangement::Column);
        assert!(frames[0][1] < frames[1][1]);
        let (arrangement, frames) = layout((1600.0, 900.0), (1.5, 1.5));
        assert_eq!(arrangement, Arrangement::Row);
        assert_eq!(frames[0][2], frames[1][2]);
        assert!(frames[0][0] + frames[0][2] + GAP <= frames[1][0] + 1.0);
        let slider = slider_frame((1600.0, 900.0), 1.5);
        assert_eq!(slider[1], (CAPTION + (900.0 - CAPTION - LABEL - slider[3]) / 2.0).round());
    }

    #[test]
    fn original_modes_keep_the_macs_keys() {
        for mode in OriginalMode::ALL {
            assert_eq!(OriginalMode::from_key(mode.key()), mode);
        }
        assert_eq!(OriginalMode::from_key("nonsense"), OriginalMode::Hold);
        assert_eq!(OriginalReference::from_key("cameraJPEG"), OriginalReference::CameraJpeg);
        assert_eq!(OriginalMode::Split.help("camera JPEG"), "Show the camera JPEG left of a line you can drag. Hold \\ to see the whole camera JPEG.");
    }
}
