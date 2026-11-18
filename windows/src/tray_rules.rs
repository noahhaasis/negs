//! The film and paper tray's rules (IMPLEMENTATION.md §4.5, §5.1, §5.2):
//! `Tray.swift`'s `StockTrayRules` and `TrayVisit`, and the keys of
//! `EditorWindowController.trayKeys`. The order of the tiles, stepping
//! through them, search, the row's geometry and hit areas, the foot's
//! words, what keeping or going back does to Undo, and Esc's order.
//!
//! No Win32 and no app types, so its tests run on any OS:
//!   cargo test --manifest-path windows/tray-rules/Cargo.toml --target-dir target/tray-rules

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

/// Something is typed in Search (spaces alone don't count).
pub fn searching(query: &str) -> bool {
    !query.trim().is_empty()
}

/// The groups as shown: the favourites first under their own title (each
/// also stays in its group), hidden while searching. `matched` are the
/// groups already filtered for `query` (`state::matching`), empty ones gone.
pub fn sections(matched: Vec<Section>, favourites: Vec<Entry>, query: &str) -> Vec<Section> {
    let matched: Vec<Section> = matched.into_iter().filter(|s| !s.stocks.is_empty()).collect();
    if searching(query) || favourites.is_empty() {
        return matched;
    }
    std::iter::once(Section { title: FAVOURITES.into(), stocks: favourites, favourites: true }).chain(matched).collect()
}

/// A tile in the row: its stock, and whether it's the favourites' copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub stock: String,
    pub favourite: bool,
}

/// The order ← → step through: the tiles as shown, the favourites first.
pub fn places(sections: &[Section]) -> Vec<Place> {
    sections.iter().flat_map(|s| s.stocks.iter().map(|e| Place { stock: e.id.clone(), favourite: s.favourites })).collect()
}

/// The tile `offset` along from `current` (its favourites' copy if
/// `in_favourites` and it has one), held at either end; from a stock not
/// in the row, the first. A copy of the same stock next to it, the last
/// favourite's own in the first group, is passed over, so every step
/// tries another stock.
pub fn step(current: &str, in_favourites: bool, offset: i32, places: &[Place]) -> Option<Place> {
    if places.is_empty() {
        return None;
    }
    let exact = places.iter().position(|p| p.stock == current && p.favourite == in_favourites);
    let Some(mut index) = exact.or_else(|| places.iter().position(|p| p.stock == current)) else {
        return places.first().cloned();
    };
    let direction: isize = if offset < 0 { -1 } else { 1 };
    for _ in 0..offset.unsigned_abs() {
        let mut next = index as isize + direction;
        while next >= 0 && (next as usize) < places.len() && places[next as usize].stock == places[index].stock {
            next += direction;
        }
        if next < 0 || next as usize >= places.len() {
            break;
        }
        index = next as usize;
    }
    places.get(index).cloned()
}

// MARK: The row's geometry, in DIPs from the row's left edge

/// A section's width: its title and its tiles.
pub fn width(section: &Section) -> f64 {
    let count = section.stocks.len() as f64;
    TITLE + TITLE_GAP + count * TILE + (count - 1.0).max(0.0) * TILE_GAP
}

/// Where each section starts in the row.
pub fn starts(sections: &[Section]) -> Vec<f64> {
    let mut x = INSET;
    sections
        .iter()
        .map(|s| {
            let start = x;
            x += width(s) + GROUP_GAP;
            start
        })
        .collect()
}

/// The row's whole width.
pub fn content_width(sections: &[Section]) -> f64 {
    sections.iter().map(width).sum::<f64>() + 2.0 * INSET + (sections.len().max(1) - 1) as f64 * GROUP_GAP
}

/// The left edge of the `index`th tile of the section starting at `start`.
pub fn tile_left(start: f64, index: usize) -> f64 {
    start + TITLE + TITLE_GAP + index as f64 * (TILE + TILE_GAP)
}

/// The left edge of `stock`'s tile in `section`.
pub fn tile_x(stock: &str, section: usize, sections: &[Section]) -> Option<f64> {
    let index = sections.get(section)?.stocks.iter().position(|e| e.id == stock)?;
    Some(tile_left(starts(sections)[section], index))
}

/// The section a stock's tile is shown in: with `favourites_first` (as the
/// tray opens, or once stepped or clicked to among the favourites) the
/// favourites' copy of a favourite, else its group.
pub fn section_of(stock: &str, sections: &[Section], favourites_first: bool) -> Option<usize> {
    let has = |s: &Section| s.stocks.iter().any(|e| e.id == stock);
    if favourites_first && let Some(index) = sections.iter().position(|s| s.favourites && has(s)) {
        return Some(index);
    }
    sections.iter().position(|s| !s.favourites && has(s))
}

/// The group in view, underlined in the foot: the last one starting within
/// 40 of the row's left edge.
pub fn current(scrolled_to: f64, starts: &[f64]) -> usize {
    starts.iter().rposition(|&s| s <= scrolled_to + 40.0).unwrap_or(0)
}

/// The scroll offset that centres a tile at `tile_x` in a row `visible` wide.
pub fn centring(tile_x: f64, visible: f64, content: f64) -> f64 {
    clamp(tile_x - (visible - TILE) / 2.0, visible, content)
}

/// The scroll offset that puts a group 30 in from the left, as the foot's index jumps.
pub fn jump(start: f64, visible: f64, content: f64) -> f64 {
    clamp(start - 30.0, visible, content)
}

pub fn clamp(x: f64, visible: f64, content: f64) -> f64 {
    x.max(0.0).min((content - visible).max(0.0))
}

/// The offset after scrolling `offset` to show `stock`'s tile: centred,
/// or (not `centring`) moved only if it's partly out of view.
pub fn reveal(offset: f64, stock: &str, in_favourites: bool, centre: bool, sections: &[Section], visible: f64) -> f64 {
    let Some(section) = section_of(stock, sections, in_favourites) else { return offset };
    let Some(x) = tile_x(stock, section, sections) else { return offset };
    if !centre && x >= offset && x + TILE <= offset + visible {
        return offset;
    }
    centring(x, visible, content_width(sections))
}

/// The offset after a wheel turn: a vertical wheel scrolls the row
/// sideways, as a sideways one does. `delta` in Windows' wheel units (120
/// a notch), positive away from the user or to the right.
pub fn wheel(offset: f64, delta: i32, sideways: bool, visible: f64, content: f64) -> f64 {
    let step = delta as f64 / 120.0 * WHEEL_NOTCH;
    // Rolling away moves back along the row, as a list scrolls up; tilting right moves on.
    clamp(if sideways { offset + step } else { offset - step }, visible, content)
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
    if y < TOP || y >= TOP + PICTURE + NAME_GAP + NAME_LINE {
        return None;
    }
    for (index, (s, start)) in sections.iter().zip(starts(sections)).enumerate() {
        for i in 0..s.stocks.len() {
            let left = tile_left(start, i);
            if x >= left && x < left + TILE {
                let star = star_rect(left);
                if x >= star.0 && x < star.0 + star.2 && y >= star.1 && y < star.1 + star.3 {
                    return Some(Hit::Star(index, i));
                }
                return Some(Hit::Tile(index, i));
            }
        }
    }
    None
}

/// The star's place (x, y, w, h) on a tile whose left edge is `left`.
pub fn star_rect(left: f64) -> (f64, f64, f64, f64) {
    (left + TILE - STAR_INSET - STAR, TOP + STAR_INSET, STAR, STAR)
}

// MARK: Words

/// The star's tooltip.
pub fn star_help(favourite: bool) -> &'static str {
    if favourite { "Remove from favourites" } else { "Add to favourites" }
}

/// The foot's hint: how to try before anything is tried, how to keep or go back after.
pub fn hint(before: Option<&str>) -> String {
    match before {
        None => "click to try · ← → to step".into(),
        Some(name) => format!("return keeps · esc goes back to {name}"),
    }
}

/// While searching the index gives way to the count.
pub fn matches(count: usize) -> String {
    match count {
        0 => String::new(),
        1 => "1 match · return tries the first".into(),
        n => format!("{n} matches · return tries the first"),
    }
}

pub fn no_match(query: &str, paper: bool) -> String {
    format!("No {} match \u{201c}{}\u{201d}", if paper { "papers" } else { "films" }, query.trim())
}

/// Done's tooltip.
pub fn done_help(paper: bool) -> String {
    format!("Keep this {} (Return)", if paper { "paper" } else { "film" })
}

/// The paper tray's switch for the paper lock.
pub fn keep_paper_words(kept: bool) -> &'static str {
    if kept { "Kept for every film" } else { "Keep for every film" }
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
}

impl<S> Visit<S> {
    pub fn new(paper: bool, before: S, before_id: impl Into<String>) -> Self {
        Self { paper, before, before_id: before_id.into(), query: String::new(), in_favourites: true }
    }

    /// The undo name for keeping a stock.
    pub fn action(&self) -> &'static str {
        if self.paper { "Change Paper" } else { "Change Film" }
    }

    /// Something else than the stock it opened on is being tried.
    pub fn tried(&self, current: &str) -> bool {
        current != self.before_id
    }

    /// The tile of `stock` says "before": another one is being tried.
    pub fn shows_before(&self, stock: &str, current: &str) -> bool {
        self.tried(current) && stock == self.before_id
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
    if before == *now {
        Ending::Nothing
    } else if keep {
        Ending::Step(before, action)
    } else {
        Ending::Restore(before)
    }
}

/// Return in Search: tries the first match; with nothing typed it keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submit {
    Keep,
    TryFirst,
}

pub fn submit(query: &str) -> Submit {
    if searching(query) { Submit::TryFirst } else { Submit::Keep }
}

/// Esc in Search: clears it; in an empty Search it goes back, as anywhere in the tray.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cancel {
    Clear,
    GoBack,
}

pub fn cancel(query: &str) -> Cancel {
    if query.is_empty() { Cancel::GoBack } else { Cancel::Clear }
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
    if command || typing {
        return TrayKey::Pass;
    }
    match key {
        Key::Escape => TrayKey::GoBack,
        Key::Return => TrayKey::Keep,
        Key::Left => TrayKey::Step(-1),
        Key::Right => TrayKey::Step(1),
        Key::Other => TrayKey::Pass,
    }
}

/// What's open when Esc is pressed (§5.2).
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
    if c.alert {
        Escape::Alert
    } else if c.typing {
        Escape::Typing
    } else if c.tray {
        Escape::Tray
    } else if c.cropping {
        Escape::Crop
    } else if c.grid {
        Escape::Grid
    } else if c.image_only {
        Escape::ImageOnly
    } else if c.photograph {
        Escape::ShowGrid
    } else {
        Escape::Nothing
    }
}

// MARK: The stand-in

/// The size a tried stock's tile is laid out at on the canvas until its
/// develop is in: its own shape, with the long side of the photograph
/// (`photo`, after cropping), else 2048 as a RAW's (`standInSize`).
pub fn stand_in_size(image: (u32, u32), photo: (f64, f64)) -> (f64, f64) {
    let long = photo.0.max(photo.1);
    let long = if long > 0.0 { long } else { 2048.0 };
    let scale = long / image.0.max(image.1).max(1) as f64;
    (image.0 as f64 * scale, image.1 as f64 * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(title: &str, ids: &[&str]) -> Section {
        Section::group(title, ids.iter().map(|id| Entry::new(*id, id.to_uppercase())).collect())
    }

    fn entries(ids: &[&str]) -> Vec<Entry> {
        ids.iter().map(|id| Entry::new(*id, id.to_uppercase())).collect()
    }

    /// The Mac's `StockGroup.matching`, for the tests: every word in the
    /// name, id or group.
    fn matching(query: &str, groups: &[Section]) -> Vec<Section> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        groups
            .iter()
            .map(|g| Section {
                stocks: g.stocks.iter().filter(|e| words.iter().all(|w| format!("{} {} {}", e.name, e.id, g.title).to_lowercase().contains(w))).cloned().collect(),
                ..g.clone()
            })
            .collect()
    }

    fn row() -> Vec<Section> {
        sections(vec![group("Color Negative", &["gold", "portra", "ektar"]), group("Black & White", &["hp5", "trix"])], entries(&["portra", "trix"]), "")
    }

    #[test]
    fn favourites_lead_under_their_own_title_and_stay_in_their_groups() {
        let row = row();
        assert_eq!(row.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Favourites", "Color Negative", "Black & White"]);
        assert!(row[0].favourites && !row[1].favourites);
        assert_eq!(places(&row).iter().filter(|p| p.stock == "portra").count(), 2);
    }

    #[test]
    fn searching_hides_the_favourites_and_empty_groups() {
        let groups = vec![group("Color Negative", &["gold", "portra"]), group("Black & White", &["hp5"])];
        let shown = sections(matching("portra", &groups), entries(&["portra"]), "portra");
        assert_eq!(shown, vec![group("Color Negative", &["portra"])]);
        // Spaces alone aren't a search.
        assert_eq!(sections(matching("  ", &groups), entries(&["portra"]), "  ")[0].title, FAVOURITES);
        assert!(sections(matching("nothing", &groups), vec![], "nothing").is_empty());
        assert_eq!(no_match(" nothing ", false), "No films match \u{201c}nothing\u{201d}");
        assert_eq!(matches(0), "");
        assert_eq!(matches(1), "1 match · return tries the first");
        assert_eq!(matches(3), "3 matches · return tries the first");
    }

    #[test]
    fn stepping_goes_through_the_favourites_and_on_into_the_groups() {
        let p = places(&row());
        // From the favourites' copy of Portra: on to Trix, the next favourite.
        assert_eq!(step("portra", true, 1, &p), Some(Place { stock: "trix".into(), favourite: true }));
        // Trix is the last favourite; the first group's Gold comes next.
        assert_eq!(step("trix", true, 1, &p), Some(Place { stock: "gold".into(), favourite: false }));
        // Back from Gold to the favourites.
        assert_eq!(step("gold", false, -1, &p), Some(Place { stock: "trix".into(), favourite: true }));
        // From Portra's own copy in its group: Gold and Ektar either side.
        assert_eq!(step("portra", false, 1, &p).unwrap().stock, "ektar");
        assert_eq!(step("portra", false, -1, &p).unwrap().stock, "gold");
        // Held at either end.
        assert_eq!(step("portra", true, -1, &p), Some(Place { stock: "portra".into(), favourite: true }));
        assert_eq!(step("trix", false, 1, &p), Some(Place { stock: "trix".into(), favourite: false }));
        // Not in the row: the first.
        assert_eq!(step("velvia", false, 1, &p).unwrap().stock, "portra");
        assert_eq!(step("x", false, 1, &[]), None);
    }

    #[test]
    fn a_favourite_next_to_its_own_copy_is_passed_over() {
        // The last favourite is the first group's first stock.
        let row = sections(vec![group("Color Negative", &["gold", "portra"])], entries(&["gold"]), "");
        let p = places(&row);
        assert_eq!(step("gold", true, 1, &p).unwrap().stock, "portra");
        assert_eq!(step("portra", false, -1, &p), Some(Place { stock: "gold".into(), favourite: false }));
        assert_eq!(step("gold", false, -1, &p), Some(Place { stock: "gold".into(), favourite: false }), "held: the copy before it is the same stock");
    }

    #[test]
    fn stepping_while_searching_goes_through_the_matches() {
        let groups = vec![group("Color Negative", &["gold", "portra160", "portra400"]), group("Motion Picture", &["vision", "portra800"])];
        let row = sections(matching("portra", &groups), entries(&["gold"]), "portra");
        let p = places(&row);
        assert_eq!(p.iter().map(|p| p.stock.as_str()).collect::<Vec<_>>(), ["portra160", "portra400", "portra800"]);
        assert_eq!(step("portra400", true, 1, &p).unwrap().stock, "portra800");
        // The stock tried before the search isn't a match: the first.
        assert_eq!(step("gold", true, 1, &p).unwrap().stock, "portra160");
    }

    #[test]
    fn the_row_is_laid_out_as_on_the_mac() {
        let row = row();
        assert_eq!(width(&row[0]), 18.0 + 9.0 + 2.0 * 212.0 + 8.0);
        let s = starts(&row);
        assert_eq!(s[0], INSET);
        assert_eq!(s[1], INSET + width(&row[0]) + GROUP_GAP);
        assert_eq!(content_width(&row), 2.0 * INSET + row.iter().map(width).sum::<f64>() + 2.0 * GROUP_GAP);
        assert_eq!(content_width(&[]), 2.0 * INSET);
        assert_eq!(tile_x("ektar", 1, &row), Some(s[1] + 27.0 + 2.0 * 220.0));
        assert_eq!(tile_x("ektar", 0, &row), None);
        assert_eq!(section_of("portra", &row, true), Some(0));
        assert_eq!(section_of("portra", &row, false), Some(1));
        assert_eq!(section_of("gold", &row, true), Some(1));
        assert_eq!(current(0.0, &s), 0);
        assert_eq!(current(s[1] - 40.0, &s), 1);
        assert_eq!(current(s[1] - 41.0, &s), 0);
    }

    #[test]
    fn opening_centres_the_current_tile_and_steps_only_reveal_it() {
        let row = row();
        let content = content_width(&row);
        let x = tile_x("ektar", 1, &row).unwrap();
        assert_eq!(reveal(0.0, "ektar", true, true, &row, 600.0), clamp(x - (600.0 - TILE) / 2.0, 600.0, content));
        // In view already: a step leaves the row alone.
        let offset = x - 100.0;
        assert_eq!(reveal(offset, "ektar", true, false, &row, 600.0), offset);
        // Out of view: centred.
        assert_eq!(reveal(0.0, "trix", false, false, &row, 600.0), centring(tile_x("trix", 2, &row).unwrap(), 600.0, content));
        // Held within the row.
        assert_eq!(centring(5.0, 600.0, content), 0.0);
        assert_eq!(clamp(1.0e6, 600.0, content), content - 600.0);
        assert_eq!(clamp(50.0, 600.0, 400.0), 0.0);
        assert_eq!(jump(starts(&row)[1], 600.0, content), starts(&row)[1] - 30.0);
    }

    #[test]
    fn the_wheel_scrolls_sideways() {
        assert_eq!(wheel(200.0, 120, false, 600.0, 2000.0), 200.0 - WHEEL_NOTCH);
        assert_eq!(wheel(200.0, -240, false, 600.0, 2000.0), 200.0 + 2.0 * WHEEL_NOTCH);
        assert_eq!(wheel(200.0, 120, true, 600.0, 2000.0), 200.0 + WHEEL_NOTCH);
        assert_eq!(wheel(10.0, 120, false, 600.0, 2000.0), 0.0);
        assert_eq!(wheel(1390.0, -120, false, 600.0, 2000.0), 1400.0);
    }

    #[test]
    fn clicks_find_the_tile_or_its_star() {
        let row = row();
        let left = tile_left(starts(&row)[1], 1);
        assert_eq!(hit(&row, left + 10.0, TOP + 50.0), Some(Hit::Tile(1, 1)));
        assert_eq!(hit(&row, left + TILE - 10.0, TOP + 10.0), Some(Hit::Star(1, 1)));
        assert_eq!(hit(&row, left + 10.0, TOP + PICTURE + 10.0), Some(Hit::Tile(1, 1)), "the name is the tile's too");
        assert_eq!(hit(&row, left - 4.0, TOP + 50.0), None, "the gap");
        assert_eq!(hit(&row, starts(&row)[1] + 5.0, TOP + 50.0), None, "the group's title");
        assert_eq!(hit(&row, left + 10.0, 2.0), None);
        assert_eq!(hit(&row, left + 10.0, ROW - 1.0), None);
    }

    #[test]
    fn the_foot_says_how_to_try_then_how_to_keep_or_go_back() {
        assert_eq!(hint(None), "click to try · ← → to step");
        assert_eq!(hint(Some("Kodak Gold 200")), "return keeps · esc goes back to Kodak Gold 200");
        assert_eq!(done_help(true), "Keep this paper (Return)");
        assert_eq!(keep_paper_words(true), "Kept for every film");
        assert_eq!(star_help(true), "Remove from favourites");
        let visit = Visit::new(false, (), "gold");
        assert!(!visit.shows_before("gold", "gold"));
        assert!(visit.shows_before("gold", "portra"));
        assert!(!visit.shows_before("portra", "portra"));
        assert_eq!(visit.action(), "Change Film");
        assert_eq!(Visit::new(true, (), "x").action(), "Change Paper");
    }

    /// A visit's tries, as the model makes them: tries replace the edit
    /// with no undo step; keeping is one step from the edit before the
    /// visit, going back restores it with none.
    struct Session {
        state: &'static str,
        undo: Vec<(&'static str, &'static str)>,
        visit: Option<Visit<&'static str>>,
    }

    impl Session {
        fn open(&mut self) {
            self.visit = Some(Visit::new(false, self.state, self.state));
        }
        fn try_stock(&mut self, stock: &'static str) {
            assert!(self.visit.is_some());
            self.state = stock;
        }
        fn end(&mut self, keep: bool) {
            let visit = self.visit.take().unwrap();
            match ending(keep, visit.before, &self.state, visit.action()) {
                Ending::Nothing => {}
                Ending::Step(before, action) => self.undo.push((before, action)),
                Ending::Restore(before) => self.state = before,
            }
        }
        fn undo(&mut self) {
            if let Some((before, _)) = self.undo.pop() {
                self.state = before;
            }
        }
    }

    #[test]
    fn a_visit_is_one_undo_step_and_esc_goes_back() {
        let mut s = Session { state: "gold", undo: vec![], visit: None };
        s.open();
        for stock in ["portra", "ektar", "trix", "hp5"] {
            s.try_stock(stock);
        }
        s.end(true);
        assert_eq!((s.state, s.undo.clone()), ("hp5", vec![("gold", "Change Film")]), "four tries, one step");
        s.undo();
        assert_eq!(s.state, "gold", "undo undoes the whole visit");

        s.open();
        s.try_stock("portra");
        s.try_stock("ektar");
        s.end(false);
        assert_eq!((s.state, s.undo.len()), ("gold", 0), "Esc puts the stock back, without a step");

        s.open();
        s.try_stock("portra");
        s.try_stock("gold");
        s.end(true);
        assert!(s.undo.is_empty(), "back where it opened: no step");
        assert_eq!(ending(false, 1, &1, "x"), Ending::Nothing);
    }

    #[test]
    fn search_keys_try_the_first_match_or_keep_and_clear_or_go_back() {
        assert_eq!(submit("portra"), Submit::TryFirst);
        assert_eq!(submit(" "), Submit::Keep);
        assert_eq!(cancel("p"), Cancel::Clear);
        assert_eq!(cancel(""), Cancel::GoBack);
    }

    #[test]
    fn the_tray_takes_esc_return_and_the_arrows_unless_typing_or_a_command() {
        assert_eq!(tray_key(Key::Escape, false, false), TrayKey::GoBack);
        assert_eq!(tray_key(Key::Return, false, false), TrayKey::Keep);
        assert_eq!(tray_key(Key::Left, false, false), TrayKey::Step(-1));
        assert_eq!(tray_key(Key::Right, false, false), TrayKey::Step(1));
        assert_eq!(tray_key(Key::Other, false, false), TrayKey::Pass);
        assert_eq!(tray_key(Key::Escape, false, true), TrayKey::Pass, "typing keeps its keys");
        assert_eq!(tray_key(Key::Left, true, false), TrayKey::Pass, "Ctrl+← is a command");
    }

    #[test]
    fn esc_goes_to_the_first_that_applies() {
        let all = EscapeContext { alert: true, typing: true, tray: true, cropping: true, grid: true, image_only: true, photograph: true };
        assert_eq!(escape(all), Escape::Alert);
        assert_eq!(escape(EscapeContext { alert: false, ..all }), Escape::Typing);
        assert_eq!(escape(EscapeContext { alert: false, typing: false, ..all }), Escape::Tray);
        assert_eq!(escape(EscapeContext { tray: true, image_only: true, photograph: true, ..Default::default() }), Escape::Tray, "the tray before Image Only");
        assert_eq!(escape(EscapeContext { cropping: true, image_only: true, ..Default::default() }), Escape::Crop);
        assert_eq!(escape(EscapeContext { grid: true, photograph: true, ..Default::default() }), Escape::Grid);
        assert_eq!(escape(EscapeContext { image_only: true, photograph: true, ..Default::default() }), Escape::ImageOnly);
        assert_eq!(escape(EscapeContext { photograph: true, ..Default::default() }), Escape::ShowGrid);
        assert_eq!(escape(EscapeContext::default()), Escape::Nothing);
    }

    #[test]
    fn the_stand_in_has_the_photographs_long_side() {
        assert_eq!(stand_in_size((320, 213), (6016.0, 4016.0)), (6016.0, 213.0 * 6016.0 / 320.0));
        assert_eq!(stand_in_size((213, 320), (4016.0, 6016.0)), (213.0 * 6016.0 / 320.0, 6016.0));
        assert_eq!(stand_in_size((320, 160), (0.0, 0.0)), (2048.0, 1024.0));
    }
}
