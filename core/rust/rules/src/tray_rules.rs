//! windows/src/tray_rules.rs's API on the shared Odin core
//! (core/src/rules/tray): the film and paper tray's rules
//! (IMPLEMENTATION.md §4.5, §5.1, §5.2; `Tray.swift`'s `StockTrayRules` and
//! `TrayVisit`, and the keys of `EditorWindowController.trayKeys`). The
//! order of the tiles, stepping through them, search, the row's geometry
//! and hit areas, the foot's words, what keeping or going back does to
//! Undo, and Esc's order.
//!
//! What runs where:
//! - In Odin: every rule, measure and word. The row goes in as borrowed
//!   views of the sections' strings, built for each call.
//! - In Rust: the constants (a test holds them to the core's), the plain
//!   data types, and moving the host's values: `sections` and `with_scan`
//!   build their rows from what the core decides, `ending` compares the
//!   host's edits (any `S: PartialEq`) and the core decides what follows.

use negs_core_sys::sheet as sys;

fn flag(b: bool) -> u8 {
    b as u8
}

/// A tile: 212 wide, the picture at 9:5 (118 tall) and the name under it.
pub const TILE: f64 = 212.0;
pub const PICTURE: f64 = 118.0;
/// Between tiles, and between groups.
pub const TILE_GAP: f64 = 8.0;
pub const GROUP_GAP: f64 = 20.0;
/// The group's name, written up its left side, and the gap after it.
pub const TITLE: f64 = 18.0;
pub const TITLE_GAP: f64 = 9.0;
/// Room round the tiles for the pencil ring, left and right.
pub const INSET: f64 = 8.0;
/// The tiles' top, under the row's top edge.
pub const TOP: f64 = 9.0;
/// The picture to the name's line, and the name's line.
pub const NAME_GAP: f64 = 4.0;
pub const NAME_LINE: f64 = 20.0;
/// The row of tiles takes the strip's 158, the foot the status line's 28.
pub const ROW: f64 = 158.0;
pub const FOOT: f64 = 28.0;
pub const HEIGHT: f64 = ROW + FOOT;
/// The favourite's star: 20 square, 5 in from the tile's top right corner.
pub const STAR: f64 = 20.0;
pub const STAR_INSET: f64 = 5.0;
/// One notch of a mouse wheel scrolls the row this far.
pub const WHEEL_NOTCH: f64 = 80.0;

pub const FAVOURITES: &str = "Favourites";
/// Your negative's group (ticket 33), and its tile's id: never a stock.
pub const SCAN: &str = "Scan";
pub const YOUR_NEGATIVE: &str = "negs.your-negative";

/// A stock as the tray lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub name: String,
}

impl Entry {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self { id: id.into(), name: name.into() }
    }
}

/// A group of tiles under its title; the favourites are one too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub stocks: Vec<Entry>,
    pub favourites: bool,
}

impl Section {
    pub fn group(title: impl Into<String>, stocks: Vec<Entry>) -> Self {
        Self { title: title.into(), stocks, favourites: false }
    }
}

/// The row as the core reads it: views of the sections' strings, valid
/// while `sections` is borrowed.
struct Row {
    _entries: Vec<sys::negs_tray_entry_t>,
    sections: Vec<sys::negs_tray_section_t>,
}

impl Row {
    fn of(sections: &[Section]) -> Row {
        let entries: Vec<sys::negs_tray_entry_t> = sections
            .iter()
            .flat_map(|s| &s.stocks)
            .map(|e| sys::negs_tray_entry_t { id: e.id.as_ptr(), id_len: e.id.len(), name: e.name.as_ptr(), name_len: e.name.len() })
            .collect();
        let mut at = 0;
        let views = sections
            .iter()
            .map(|s| {
                let view = sys::negs_tray_section_t {
                    title: s.title.as_ptr(),
                    title_len: s.title.len(),
                    stocks: entries[at..].as_ptr(),
                    count: s.stocks.len(),
                    favourites: flag(s.favourites),
                };
                at += s.stocks.len();
                view
            })
            .collect();
        Row { _entries: entries, sections: views }
    }

    fn ptr(&self) -> *const sys::negs_tray_section_t {
        self.sections.as_ptr()
    }

    fn len(&self) -> usize {
        self.sections.len()
    }
}

fn place_views(places: &[Place]) -> Vec<sys::negs_tray_place_t> {
    places.iter().map(|p| sys::negs_tray_place_t { stock: p.stock.as_ptr(), stock_len: p.stock.len(), favourite: flag(p.favourite) }).collect()
}

/// Something is typed in Search (spaces alone don't count).
pub fn searching(query: &str) -> bool {
    unsafe { sys::negs_tray_searching(query.as_ptr(), query.len()) != 0 }
}

/// The groups as shown: the favourites first under their own title (each
/// also stays in its group), hidden while searching. `matched` are the
/// groups already filtered for `query` (`state::matching`), empty ones gone.
pub fn sections(matched: Vec<Section>, favourites: Vec<Entry>, query: &str) -> Vec<Section> {
    let counts: Vec<usize> = matched.iter().map(|s| s.stocks.len()).collect();
    let mut kept = vec![0usize; counts.len()];
    let mut lead = 0u8;
    let n = unsafe { sys::negs_tray_sections(counts.as_ptr(), counts.len(), favourites.len(), query.as_ptr(), query.len(), kept.as_mut_ptr(), &mut lead) };
    kept.truncate(n);
    let mut out = Vec::with_capacity(n + 1);
    if lead != 0 {
        out.push(Section { title: FAVOURITES.into(), stocks: favourites, favourites: true });
    }
    let mut matched: Vec<Option<Section>> = matched.into_iter().map(Some).collect();
    out.extend(kept.into_iter().map(|i| matched[i].take().expect("each group once")));
    out
}

/// The film tray's row: Your negative first, in its own Scan group, so the
/// order never changes from one photograph to the next; not while
/// searching, nor for papers or the films a negative is read as.
pub fn with_scan(sections: Vec<Section>, paper: bool, read_as: bool, query: &str, name: &str) -> Vec<Section> {
    if unsafe { sys::negs_tray_with_scan(flag(paper), flag(read_as), query.as_ptr(), query.len()) } == 0 {
        return sections;
    }
    std::iter::once(Section::group(SCAN, vec![Entry::new(YOUR_NEGATIVE, name)])).chain(sections).collect()
}

/// A tile in the row: its stock, and whether it's the favourites' copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub stock: String,
    pub favourite: bool,
}

/// The order ← → step through: the tiles as shown, the favourites first.
pub fn places(sections: &[Section]) -> Vec<Place> {
    let row = Row::of(sections);
    let total = sections.iter().map(|s| s.stocks.len()).sum();
    let mut out: Vec<sys::negs_tray_place_t> = Vec::with_capacity(total);
    unsafe {
        let n = sys::negs_tray_places(row.ptr(), row.len(), out.as_mut_ptr(), total);
        out.set_len(n.min(total));
    }
    out.iter()
        .map(|p| {
            // The views point into `sections`' own ids.
            let bytes = unsafe { std::slice::from_raw_parts(p.stock, p.stock_len) };
            Place { stock: std::str::from_utf8(bytes).expect("an id").to_string(), favourite: p.favourite != 0 }
        })
        .collect()
}

/// The tile `offset` along from `current` (its favourites' copy if
/// `in_favourites` and it has one), held at either end; from a stock not
/// in the row, the first. A copy of the same stock next to it, the last
/// favourite's own in the first group, is passed over, so every step
/// tries another stock.
pub fn step(current: &str, in_favourites: bool, offset: i32, places: &[Place]) -> Option<Place> {
    let views = place_views(places);
    let i = unsafe { sys::negs_tray_step(current.as_ptr(), current.len(), flag(in_favourites), offset, views.as_ptr(), views.len()) };
    (i >= 0).then(|| places[i as usize].clone())
}

// MARK: The row's geometry, in DIPs from the row's left edge

/// A section's width: its title and its tiles.
pub fn width(section: &Section) -> f64 {
    unsafe { sys::negs_tray_width(section.stocks.len()) }
}

/// Where each section starts in the row.
pub fn starts(sections: &[Section]) -> Vec<f64> {
    let row = Row::of(sections);
    let mut out = vec![0.0; sections.len()];
    unsafe { sys::negs_tray_starts(row.ptr(), row.len(), out.as_mut_ptr()) };
    out
}

/// The row's whole width.
pub fn content_width(sections: &[Section]) -> f64 {
    let row = Row::of(sections);
    unsafe { sys::negs_tray_content_width(row.ptr(), row.len()) }
}

/// The left edge of the `index`th tile of the section starting at `start`.
pub fn tile_left(start: f64, index: usize) -> f64 {
    unsafe { sys::negs_tray_tile_left(start, index) }
}

/// The left edge of `stock`'s tile in `section`.
pub fn tile_x(stock: &str, section: usize, sections: &[Section]) -> Option<f64> {
    let row = Row::of(sections);
    let mut x = 0.0;
    let found = unsafe { sys::negs_tray_tile_x(stock.as_ptr(), stock.len(), section, row.ptr(), row.len(), &mut x) };
    (found != 0).then_some(x)
}

/// The section a stock's tile is shown in: with `favourites_first` (as the
/// tray opens, or once stepped or clicked to among the favourites) the
/// favourites' copy of a favourite, else its group.
pub fn section_of(stock: &str, sections: &[Section], favourites_first: bool) -> Option<usize> {
    let row = Row::of(sections);
    let i = unsafe { sys::negs_tray_section_of(stock.as_ptr(), stock.len(), row.ptr(), row.len(), flag(favourites_first)) };
    (i >= 0).then_some(i as usize)
}

/// The group in view, underlined in the foot: the last one starting within
/// 40 of the row's left edge.
pub fn current(scrolled_to: f64, starts: &[f64]) -> usize {
    unsafe { sys::negs_tray_current(scrolled_to, starts.as_ptr(), starts.len()) }
}

/// The scroll offset that centres a tile at `tile_x` in a row `visible` wide.
pub fn centring(tile_x: f64, visible: f64, content: f64) -> f64 {
    unsafe { sys::negs_tray_centring(tile_x, visible, content) }
}

/// The scroll offset that puts a group 30 in from the left, as the foot's index jumps.
pub fn jump(start: f64, visible: f64, content: f64) -> f64 {
    unsafe { sys::negs_tray_jump(start, visible, content) }
}

pub fn clamp(x: f64, visible: f64, content: f64) -> f64 {
    unsafe { sys::negs_tray_clamp(x, visible, content) }
}

/// The offset after scrolling `offset` to show `stock`'s tile: centred,
/// or (not `centring`) moved only if it's partly out of view.
pub fn reveal(offset: f64, stock: &str, in_favourites: bool, centre: bool, sections: &[Section], visible: f64) -> f64 {
    let row = Row::of(sections);
    unsafe { sys::negs_tray_reveal(offset, stock.as_ptr(), stock.len(), flag(in_favourites), flag(centre), row.ptr(), row.len(), visible) }
}

/// The offset after a wheel turn: a vertical wheel scrolls the row
/// sideways, as a sideways one does. `delta` in Windows' wheel units (120
/// a notch), positive away from the user or to the right.
pub fn wheel(offset: f64, delta: i32, sideways: bool, visible: f64, content: f64) -> f64 {
    unsafe { sys::negs_tray_wheel(offset, delta, flag(sideways), visible, content) }
}

/// A group clicked in the foot glides there over 0.25 s (`TrayScroll`).
pub const GLIDE_MS: f64 = 250.0;

/// The offset `elapsed_ms` into a glide from `from` to `to`, eased in and
/// out; `to` once it's over.
pub fn glide(from: f64, to: f64, elapsed_ms: f64) -> f64 {
    unsafe { sys::negs_tray_glide(from, to, elapsed_ms) }
}

/// What's under a point of the row, in DIPs from its left edge (with the
/// scroll added) and from its top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A tile: its section and index.
    Tile(usize, usize),
    /// A tile's star.
    Star(usize, usize),
}

pub fn hit(sections: &[Section], x: f64, y: f64) -> Option<Hit> {
    let row = Row::of(sections);
    let (mut section, mut index) = (0, 0);
    match unsafe { sys::negs_tray_hit(row.ptr(), row.len(), x, y, &mut section, &mut index) } {
        sys::NEGS_TRAY_HIT_TILE => Some(Hit::Tile(section, index)),
        sys::NEGS_TRAY_HIT_STAR => Some(Hit::Star(section, index)),
        _ => None,
    }
}

/// The star's place (x, y, w, h) on a tile whose left edge is `left`.
pub fn star_rect(left: f64) -> (f64, f64, f64, f64) {
    let mut r = [0.0; 4];
    unsafe { sys::negs_tray_star_rect(left, &mut r) };
    (r[0], r[1], r[2], r[3])
}

// MARK: Words

fn static_str(s: *const std::os::raw::c_char) -> &'static str {
    unsafe { sys::static_str(s) }
}

/// The star's tooltip.
pub fn star_help(favourite: bool) -> &'static str {
    static_str(unsafe { sys::negs_tray_star_help(flag(favourite)) })
}

/// What a screen reader says for a tile's star (`starLabel`).
pub fn star_label(name: &str, favourite: bool) -> String {
    sys::text(|out, cap| unsafe { sys::negs_tray_star_label(name.as_ptr(), name.len(), flag(favourite), out, cap) })
}

/// A tile's accessible value: the one tried, or the one the tray opened on.
pub fn tile_value(selected: bool, before: bool) -> &'static str {
    static_str(unsafe { sys::negs_tray_tile_value(flag(selected), flag(before)) })
}

/// The foot's group names, as a screen reader says them.
pub fn index_label(title: &str) -> String {
    sys::text(|out, cap| unsafe { sys::negs_tray_index_label(title.as_ptr(), title.len(), out, cap) })
}

/// The paper lock's switch, as a screen reader says it.
pub const KEEP_PAPER_LABEL: &str = "Keep paper when changing film";

/// The foot's hint: how to try before anything is tried, how to keep or go back after.
pub fn hint(before: Option<&str>) -> String {
    let name = before.unwrap_or("");
    sys::text(|out, cap| unsafe { sys::negs_tray_hint(flag(before.is_some()), name.as_ptr(), name.len(), out, cap) })
}

/// While searching the index gives way to the count.
pub fn matches(count: usize) -> String {
    sys::text(|out, cap| unsafe { sys::negs_tray_matches(count, out, cap) })
}

pub fn no_match(query: &str, paper: bool) -> String {
    sys::text(|out, cap| unsafe { sys::negs_tray_no_match(query.as_ptr(), query.len(), flag(paper), out, cap) })
}

/// Done's tooltip.
pub fn done_help(paper: bool) -> String {
    static_str(unsafe { sys::negs_tray_done_help(flag(paper)) }).to_string()
}

/// The paper tray's switch for the paper lock.
pub fn keep_paper_words(kept: bool) -> &'static str {
    static_str(unsafe { sys::negs_tray_keep_paper_words(flag(kept)) })
}

// MARK: A visit: trying, keeping and going back

/// One visit to the film or paper tray: the edit when it opened, so Esc
/// can go back to it and keeping a stock is one undo step from it.
#[derive(Clone, Debug, PartialEq)]
pub struct Visit<S> {
    pub paper: bool,
    /// The edit when the tray opened.
    pub before: S,
    /// The stock the tray opened on.
    pub before_id: String,
    /// Films only.
    pub query: String,
    /// The stock being tried is its copy among the favourites (where the
    /// tray opens on a favourite, or the copy last clicked or stepped to),
    /// so ← → go on from there rather than from its group.
    pub in_favourites: bool,
    /// The film a negative is read as (its "read as" name): the colour
    /// negatives only, the paper and the negative kept.
    pub read_as: bool,
    /// The film tray of a negative, or of a photograph the helper found
    /// looks like one: it opens on Scan, at the row's start.
    pub opens_on_scan: bool,
}

impl<S> Visit<S> {
    pub fn new(paper: bool, before: S, before_id: impl Into<String>) -> Self {
        Self { paper, before, before_id: before_id.into(), query: String::new(), in_favourites: true, read_as: false, opens_on_scan: false }
    }

    /// The undo name for keeping a stock.
    pub fn action(&self) -> &'static str {
        static_str(unsafe { sys::negs_tray_visit_action(flag(self.paper), flag(self.read_as)) })
    }

    /// Something else than the stock it opened on is being tried.
    pub fn tried(&self, current: &str) -> bool {
        let b = &self.before_id;
        unsafe { sys::negs_tray_visit_tried(b.as_ptr(), b.len(), current.as_ptr(), current.len()) != 0 }
    }

    /// The tile of `stock` says "before": another one is being tried.
    pub fn shows_before(&self, stock: &str, current: &str) -> bool {
        let b = &self.before_id;
        unsafe { sys::negs_tray_visit_shows_before(b.as_ptr(), b.len(), stock.as_ptr(), stock.len(), current.as_ptr(), current.len()) != 0 }
    }
}

/// What ending a visit does to the photograph and to Undo.
#[derive(Clone, Debug, PartialEq)]
pub enum Ending<S> {
    /// Kept what it opened on, or went back to it: nothing.
    Nothing,
    /// Kept another stock: one undo step back to the edit before the visit.
    Step(S, &'static str),
    /// Went back: the edit before the visit returns, without an undo step.
    Restore(S),
}

/// Return, Done, a click outside, a command or any other edit keep the
/// stock being tried (`keep`); Esc goes back. `before` is the visit's edit
/// as it compares with `now` (with the app's preferences in it).
pub fn ending<S: PartialEq>(keep: bool, before: S, now: &S, action: &'static str) -> Ending<S> {
    match unsafe { sys::negs_tray_ending(flag(keep), flag(before == *now)) } {
        sys::NEGS_TRAY_ENDING_STEP => Ending::Step(before, action),
        sys::NEGS_TRAY_ENDING_RESTORE => Ending::Restore(before),
        _ => Ending::Nothing,
    }
}

/// Return in Search: tries the first match; with nothing typed it keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submit {
    Keep,
    TryFirst,
}

pub fn submit(query: &str) -> Submit {
    match unsafe { sys::negs_tray_submit(query.as_ptr(), query.len()) } {
        sys::NEGS_TRAY_SUBMIT_TRY_FIRST => Submit::TryFirst,
        _ => Submit::Keep,
    }
}

/// Esc in Search: clears it; in an empty Search it goes back, as anywhere in the tray.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cancel {
    Clear,
    GoBack,
}

pub fn cancel(query: &str) -> Cancel {
    match unsafe { sys::negs_tray_cancel(query.as_ptr(), query.len()) } {
        sys::NEGS_TRAY_CANCEL_GO_BACK => Cancel::GoBack,
        _ => Cancel::Clear,
    }
}

// MARK: Keys

/// A key the tray may take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Return,
    Left,
    Right,
    Other,
}

/// What a key does while the tray is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayKey {
    /// Esc: back to the stock from before the tray opened, and close.
    GoBack,
    /// Return: keep the stock being tried, and close.
    Keep,
    /// ← →: the previous or next stock.
    Step(i32),
    /// Not the tray's: typing keeps its keys, and Ctrl or Alt are commands
    /// and menus (which keep the stock first, `App::command`).
    Pass,
}

/// While the tray is open, Esc goes back, Return keeps, and ← → step
/// through the stocks rather than the photographs. Typing keeps its keys.
pub fn tray_key(key: Key, command: bool, typing: bool) -> TrayKey {
    let k = match key {
        Key::Escape => sys::NEGS_TRAY_KEY_ESCAPE,
        Key::Return => sys::NEGS_TRAY_KEY_RETURN,
        Key::Left => sys::NEGS_TRAY_KEY_LEFT,
        Key::Right => sys::NEGS_TRAY_KEY_RIGHT,
        Key::Other => sys::NEGS_TRAY_KEY_OTHER,
    };
    let mut step = 0;
    match unsafe { sys::negs_tray_key(k, flag(command), flag(typing), &mut step) } {
        sys::NEGS_TRAY_ACTION_GO_BACK => TrayKey::GoBack,
        sys::NEGS_TRAY_ACTION_KEEP => TrayKey::Keep,
        sys::NEGS_TRAY_ACTION_STEP => TrayKey::Step(step),
        _ => TrayKey::Pass,
    }
}

/// What's open when Esc is pressed (§5.2); negs_tray_escape_context_t's
/// layout, so it crosses as it is.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EscapeContext {
    pub alert: bool,
    pub typing: bool,
    pub tray: bool,
    pub cropping: bool,
    pub grid: bool,
    pub image_only: bool,
    pub photograph: bool,
}

/// What Esc does, the first that applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escape {
    /// The alert closes.
    Alert,
    /// The field puts the old value back.
    Typing,
    /// The tray goes back to the earlier stock and closes.
    Tray,
    /// Cropping cancels.
    Crop,
    /// The grid folds back.
    Grid,
    /// Image Only ends.
    ImageOnly,
    /// The photograph view opens the grid (on Windows once it has one).
    ShowGrid,
    Nothing,
}

pub fn escape(c: EscapeContext) -> Escape {
    match unsafe { sys::negs_tray_escape((&c as *const EscapeContext).cast()) } {
        sys::NEGS_ESCAPE_ALERT => Escape::Alert,
        sys::NEGS_ESCAPE_TYPING => Escape::Typing,
        sys::NEGS_ESCAPE_TRAY => Escape::Tray,
        sys::NEGS_ESCAPE_CROP => Escape::Crop,
        sys::NEGS_ESCAPE_GRID => Escape::Grid,
        sys::NEGS_ESCAPE_IMAGE_ONLY => Escape::ImageOnly,
        sys::NEGS_ESCAPE_SHOW_GRID => Escape::ShowGrid,
        _ => Escape::Nothing,
    }
}

// MARK: The stand-in

/// The size a tried stock's tile is laid out at on the canvas until its
/// develop is in: its own shape, with the long side of the photograph
/// (`photo`, after cropping), else 2048 as a RAW's (`standInSize`).
pub fn stand_in_size(image: (u32, u32), photo: (f64, f64)) -> (f64, f64) {
    let mut out = [0.0; 2];
    unsafe { sys::negs_tray_stand_in_size(image.0, image.1, photo.0, photo.1, &mut out) };
    (out[0], out[1])
}

#[cfg(test)]
mod windows_tests;

#[cfg(test)]
mod core_tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn the_constants_are_the_cores() {
        let mut c = std::mem::MaybeUninit::<sys::negs_tray_constants_t>::zeroed();
        let c = unsafe {
            sys::negs_tray_constants(c.as_mut_ptr());
            c.assume_init()
        };
        let ours = [TILE, PICTURE, TILE_GAP, GROUP_GAP, TITLE, TITLE_GAP, INSET, TOP, NAME_GAP, NAME_LINE, ROW, FOOT, HEIGHT, STAR, STAR_INSET, WHEEL_NOTCH, GLIDE_MS];
        let core = [
            c.tile, c.picture, c.tile_gap, c.group_gap, c.title, c.title_gap, c.inset, c.top, c.name_gap, c.name_line, c.row, c.foot, c.height, c.star,
            c.star_inset, c.wheel_notch, c.glide_ms,
        ];
        for (i, (a, b)) in ours.iter().zip(core).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "constant {i}");
        }
        assert_eq!(static_str(c.favourites), FAVOURITES);
        assert_eq!(static_str(c.scan), SCAN);
        assert_eq!(static_str(c.your_negative), YOUR_NEGATIVE);
        assert_eq!(static_str(c.keep_paper_label), KEEP_PAPER_LABEL);
    }

    #[test]
    fn escape_contexts_cross_as_they_are() {
        use sys::negs_tray_escape_context_t as E;
        assert_eq!(size_of::<EscapeContext>(), size_of::<E>());
        assert_eq!((offset_of!(EscapeContext, cropping), offset_of!(EscapeContext, photograph)), (offset_of!(E, cropping), offset_of!(E, photograph)));
    }

    #[test]
    fn long_words_come_back_whole() {
        let name = "Kodak ".repeat(100);
        assert_eq!(star_label(&name, true), format!("Remove {name} from favourites"));
        assert_eq!(hint(Some(&name)), format!("return keeps · esc goes back to {name}"));
        assert_eq!(no_match("\u{3000} x\u{2028}", true), "No papers match \u{201c}x\u{201d}");
        assert!(!searching("\u{a0}\u{85} \t"));
    }
}
