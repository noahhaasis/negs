//! windows/src/compare.rs's API on the shared Odin core
//! (core/src/rules/compare): Compare (ticket 46, `Compare.swift`), two
//! photographs side by side or in one frame with a line between them, or a
//! photograph against its original; and what Original ⌄ makes the Original
//! word do. The rules and geometry.
//!
//! What runs where:
//! - In Odin: every rule, word and frame. Photographs go in as views of
//!   their paths' bytes (`OsStr::as_encoded_bytes`) and variants, so two are
//!   the same when those are byte for byte, as the core's library compares
//!   paths (Rust's `Path` would also call `a/b` and `a//b` the same; the
//!   library never makes such paths).
//! - In Rust: the plain data types and their accessors (`Side::item`,
//!   `Comparison::active` and `inactive`, `Offer`), the constants (a test
//!   holds them to the core's), and calling the host's `number` for the
//!   photographs the core's offer and word name.

use negs_core_sys::marks::negs_rect_t;
use negs_core_sys::sheet as sys;
use negs_library::Item;
use std::os::raw::c_char;

fn flag(b: bool) -> u8 {
    b as u8
}

fn static_str(s: *const c_char) -> &'static str {
    unsafe { sys::static_str(s) }
}

/// A view of `item` for the core, valid while `item` is borrowed.
fn view(item: &Item) -> sys::negs_compare_item_t {
    let photo = item.photo.as_os_str().as_encoded_bytes();
    sys::negs_compare_item_t { photo: photo.as_ptr(), photo_len: photo.len(), variant: sys::negs_compare_text_t::of(item.variant.as_deref()) }
}

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
        unsafe { sys::negs_compare_against_original(flag(self.first.is_original()), flag(self.second.is_original())) != 0 }
    }

    /// The photograph the panel edits: the active one, or the developed one
    /// beside its original.
    pub fn edited(&self) -> &Item {
        let first = unsafe { sys::negs_compare_edited_is_first(flag(self.first.is_original()), flag(self.second.is_original()), flag(self.active_first)) };
        if first != 0 { self.first.item() } else { self.second.item() }
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

fn offer_title(kind: u32, a: &str, b: &str) -> String {
    sys::text(|out, cap| unsafe { sys::negs_compare_offer_title(kind, a.as_ptr(), a.len(), b.as_ptr(), b.len(), out, cap) })
}

/// The item for a right-click on `item` acting on `targets` (the selection
/// it's in, or itself): two compare with each other, three or more can't,
/// one compares with the current photograph, or with its original.
pub fn offer(targets: &[Item], current: Option<&Item>, item: &Item, number: &dyn Fn(&Item) -> String, can_compare: bool, can_with_original: bool, original: &str) -> Offer {
    let other = current.filter(|c| *c != item);
    let mut o = sys::negs_compare_offer_t::default();
    unsafe { sys::negs_compare_offer(targets.len(), flag(other.is_some()), flag(can_compare), flag(can_with_original), &mut o) };
    let (title, pair) = match o.kind {
        sys::NEGS_COMPARE_OFFER_TOO_MANY => (offer_title(o.kind, "", ""), None),
        sys::NEGS_COMPARE_OFFER_PAIR => {
            let (first, second) = (&targets[0], &targets[1]);
            (offer_title(o.kind, &number(first), &number(second)), Some((first.clone(), second.clone())))
        }
        sys::NEGS_COMPARE_OFFER_WITH_CURRENT => {
            let current = other.expect("a current photograph");
            (offer_title(o.kind, &number(current), ""), Some((current.clone(), item.clone())))
        }
        _ => (offer_title(o.kind, original, ""), None),
    };
    Offer { title, enabled: o.enabled != 0, pair, with_original: o.with_original != 0 }
}

/// The title bar's Compare word, the C key and View › Compare: whether
/// they act on the selection (in shown order), and the word's tooltip.
pub fn word(selected: &[Item], number: &dyn Fn(&Item) -> String, can_compare: bool) -> (bool, String) {
    let kind = unsafe { sys::negs_compare_word(selected.len(), flag(can_compare)) };
    let (a, b) = if kind == sys::NEGS_COMPARE_WORD_PAIR { (number(&selected[0]), number(&selected[1])) } else { (String::new(), String::new()) };
    let title = sys::text(|out, cap| unsafe { sys::negs_compare_word_title(kind, a.as_ptr(), a.len(), b.as_ptr(), b.len(), out, cap) });
    (kind == sys::NEGS_COMPARE_WORD_PAIR, title)
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
    let facts = |f: Option<&[Option<String>; 5]>| f.map(|f| f.each_ref().map(|p| sys::negs_compare_text_t::of(p.as_deref())));
    let (a, b) = (facts(a), facts(b));
    let ptr = |f: &Option<[sys::negs_compare_text_t; 5]>| f.as_ref().map_or(std::ptr::null(), |f| f.as_ptr());
    let (pa, pb) = (sys::negs_compare_text_t::of(paper.0), sys::negs_compare_text_t::of(paper.1));
    sys::text(|out, cap| unsafe {
        sys::negs_compare_differences(ptr(&a), ptr(&b), film.0.as_ptr(), film.0.len(), film.1.as_ptr(), film.1.len(), pa, pb, out, cap)
    })
}

pub fn menu_title(comparing: bool) -> &'static str {
    static_str(unsafe { sys::negs_compare_menu_title(flag(comparing)) })
}

/// ← →: the photograph `offset` places from `from` in `order`, skipping
/// `first`; None past the ends. One no longer shown steps from the first.
pub fn step(from: &Item, first: &Item, order: &[Item], offset: isize) -> Option<Item> {
    let views: Vec<sys::negs_compare_item_t> = order.iter().map(view).collect();
    let (f, s) = (view(from), view(first));
    let i = unsafe { sys::negs_compare_step(&f, &s, views.as_ptr(), views.len(), offset as i64) };
    (i >= 0).then(|| order[i as usize].clone())
}

/// The number a photograph is known by on the roll: its place in the
/// folder, counted from 1, and a duplicate's letter after it (7a, 7b).
/// `items` is the folder's, every duplicate in order.
pub fn frame_number(items: &[Item], item: &Item) -> String {
    let views: Vec<sys::negs_compare_item_t> = items.iter().map(view).collect();
    let it = view(item);
    sys::text(|out, cap| unsafe { sys::negs_compare_frame_number(views.as_ptr(), views.len(), &it, out, cap) })
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

    fn c(self) -> u32 {
        match self {
            OriginalMode::Hold => sys::NEGS_ORIGINAL_HOLD,
            OriginalMode::Split => sys::NEGS_ORIGINAL_SPLIT,
            OriginalMode::SideBySide => sys::NEGS_ORIGINAL_SIDE_BY_SIDE,
        }
    }

    pub fn key(self) -> &'static str {
        static_str(unsafe { sys::negs_compare_original_mode_key(self.c()) })
    }

    pub fn from_key(key: &str) -> Self {
        match unsafe { sys::negs_compare_original_mode_from_key(key.as_ptr(), key.len()) } {
            sys::NEGS_ORIGINAL_SPLIT => OriginalMode::Split,
            sys::NEGS_ORIGINAL_SIDE_BY_SIDE => OriginalMode::SideBySide,
            _ => OriginalMode::Hold,
        }
    }

    pub fn title(self) -> &'static str {
        static_str(unsafe { sys::negs_compare_original_mode_title(self.c()) })
    }

    /// The Original word's tooltip; `original` is "original" or "camera JPEG".
    #[allow(dead_code)]
    pub fn help(self, original: &str) -> String {
        sys::text(|out, cap| unsafe { sys::negs_compare_original_mode_help(self.c(), original.as_ptr(), original.len(), out, cap) })
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

    fn c(self) -> u32 {
        match self {
            OriginalReference::Raw => sys::NEGS_ORIGINAL_RAW,
            OriginalReference::CameraJpeg => sys::NEGS_ORIGINAL_CAMERA_JPEG,
        }
    }

    pub fn key(self) -> &'static str {
        static_str(unsafe { sys::negs_compare_original_reference_key(self.c()) })
    }

    pub fn from_key(key: &str) -> Self {
        match unsafe { sys::negs_compare_original_reference_from_key(key.as_ptr(), key.len()) } {
            sys::NEGS_ORIGINAL_CAMERA_JPEG => OriginalReference::CameraJpeg,
            _ => OriginalReference::Raw,
        }
    }

    pub fn title(self) -> &'static str {
        static_str(unsafe { sys::negs_compare_original_reference_title(self.c()) })
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

fn frame(r: &negs_rect_t) -> Frame {
    [r.x, r.y, r.w, r.h]
}

fn frames(r: &[negs_rect_t; 2]) -> [Frame; 2] {
    [frame(&r[0]), frame(&r[1])]
}

const NO_RECT: negs_rect_t = negs_rect_t { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };

pub fn fit(aspect: f64, room: (f64, f64)) -> (f64, f64) {
    let mut out = [0.0; 2];
    unsafe { sys::negs_compare_fit(aspect, room.0, room.1, &mut out) };
    (out[0], out[1])
}

pub fn row_layout(canvas: (f64, f64), aspects: (f64, f64)) -> [Frame; 2] {
    let mut out = [NO_RECT; 2];
    unsafe { sys::negs_compare_row_layout(canvas.0, canvas.1, aspects.0, aspects.1, &mut out) };
    frames(&out)
}

pub fn column_layout(canvas: (f64, f64), aspects: (f64, f64)) -> [Frame; 2] {
    let mut out = [NO_RECT; 2];
    unsafe { sys::negs_compare_column_layout(canvas.0, canvas.1, aspects.0, aspects.1, &mut out) };
    frames(&out)
}

/// The arrangement that shows the two larger: in a row, or one above the other.
pub fn layout(canvas: (f64, f64), aspects: (f64, f64)) -> (Arrangement, [Frame; 2]) {
    let mut out = [NO_RECT; 2];
    let arrangement = unsafe { sys::negs_compare_layout(canvas.0, canvas.1, aspects.0, aspects.1, &mut out) };
    (if arrangement == sys::NEGS_COMPARE_COLUMN { Arrangement::Column } else { Arrangement::Row }, frames(&out))
}

/// The slider's one frame, at the first photograph's proportions.
pub fn slider_frame(canvas: (f64, f64), aspect: f64) -> Frame {
    let mut out = NO_RECT;
    unsafe { sys::negs_compare_slider_frame(canvas.0, canvas.1, aspect, &mut out) };
    frame(&out)
}

#[cfg(test)]
mod windows_tests;

#[cfg(test)]
mod core_tests {
    use super::*;

    #[test]
    fn the_constants_are_the_cores() {
        let mut c = std::mem::MaybeUninit::<sys::negs_compare_constants_t>::zeroed();
        let c = unsafe {
            sys::negs_compare_constants(c.as_mut_ptr());
            c.assume_init()
        };
        let ours = [CAPTION, LABEL, STACKED_CAPTION, GAP, STACKED_GAP, SIDE, BOTTOM];
        let core = [c.caption, c.label, c.stacked_caption, c.gap, c.stacked_gap, c.side, c.bottom];
        for (i, (a, b)) in ours.iter().zip(core).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "constant {i}");
        }
        assert_eq!(c.parts.map(static_str), PARTS);
    }
}
