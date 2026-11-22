//! windows/src/sheet_rules.rs's API on the shared Odin core
//! (core/src/rules/sheet): the film strip's and the grid's rules: where
//! every frame and thumbnail goes, the order thumbnails are read and
//! developed in, how many at once, the grid's sentence and foot, the fade
//! and the scroll bar. Lengths are DIPs, the Mac's points.
//!
//! What runs where:
//! - In Odin: every rule, layout, order, word and curve.
//! - In Rust: the constants (a test holds them to the core's), the plain
//!   data types and their accessors (`Rect`'s edges and tests, `Row`,
//!   `StripLayout`'s and `SheetLayout`'s fields, `PumpItem`, `Part`), the
//!   moving of host values in `outward` (the core orders their indices),
//!   and `Lru`, a generic cache of the host's values (pixels): nothing in it
//!   is a rule another front end shares, and it holds Rust values.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::os::raw::c_void;

use negs_core_sys::marks::negs_rect_t;
use negs_core_sys::sheet as sys;
use negs_library::{EditsFilter, Filter, FlagFilter, RatingFilter};

fn flag(b: bool) -> u8 {
    b as u8
}

fn index(i: i64) -> Option<usize> {
    (i >= 0).then_some(i as usize)
}

// MARK: Sizes (Theme.Size)

/// The strip: its top padding, the picture and the 12 DIP rebate under it.
pub const STRIP: f64 = 158.0;
pub const STRIP_TOP: f64 = 6.0;
pub const STRIP_REBATE: f64 = 12.0;
pub const STRIP_GAP: f64 = 5.0;
/// The picture's height: the strip less its top padding and rebate.
pub const PICTURE: f64 = STRIP - STRIP_TOP - STRIP_REBATE;
/// The status line under the strip.
pub const STATUS: f64 = 28.0;
/// The strip and the status line: as tall as the tray, so the canvas keeps
/// its size when the tray opens.
pub const BAND: f64 = STRIP + STATUS;
/// Frames other than the current one; under the pointer; pressed.
pub const STRIP_DIM: f64 = 0.42;
pub const STRIP_HOVER: f64 = 0.75;
pub const STRIP_PRESSED: f64 = 0.9;
/// The pencil under the current frame, 3 DIP into the rebate.
pub const PENCIL_WIDTH: f64 = 4.0;
pub const PENCIL_DROP: f64 = 3.0;
/// A photograph's width ÷ height before its picture is read.
pub const FALLBACK_RATIO: f64 = 1.5;
/// Frames are drawn this far either side of the strip's view.
pub const STRIP_MARGIN: f64 = 400.0;

/// The strip's handle (`StripLeader`): a tongue cut on the slant at the
/// strip's right end, risen a little under the pointer.
pub const LEADER_WIDTH: f64 = 92.0;
pub const LEADER_HEIGHT: f64 = 24.0;
pub const LEADER_RAISED: f64 = 30.0;
pub const LEADER_SLANT: f64 = 28.0;

/// A reject's picture: greyed, more in the strip (0.22, 0.5 for the current
/// frame) than in the grid (0.4) (`PhotoMarks.rejectOpacity`).
pub fn reject_opacity(in_strip: bool, current: bool) -> f64 {
    unsafe { sys::negs_sheet_reject_opacity(flag(in_strip), flag(current)) }
}

/// How the strip shows a frame: the current and selected ones whole, the
/// rest dimmed, lifted under the pointer, a reject greyed further.
pub fn strip_opacity(current: bool, selected: bool, rejected: bool, hovered: bool, pressed: bool) -> f64 {
    unsafe { sys::negs_sheet_strip_opacity(flag(current), flag(selected), flag(rejected), flag(hovered), flag(pressed)) }
}

// MARK: Rectangles

/// x, y, width, height: negs_rect_t's layout, so frames cross as they are.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    pub fn max_x(&self) -> f64 {
        self.x + self.w
    }
    pub fn max_y(&self) -> f64 {
        self.y + self.h
    }
    pub fn mid_x(&self) -> f64 {
        self.x + self.w / 2.0
    }
    pub fn mid_y(&self) -> f64 {
        self.y + self.h / 2.0
    }
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.max_x() && y >= self.y && y < self.max_y()
    }
    /// They overlap, edges touching not counted (`CGRect.intersects`).
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.max_x() && o.x < self.max_x() && self.y < o.max_y() && o.y < self.max_y()
    }
    pub fn spanning(a: (f64, f64), b: (f64, f64)) -> Rect {
        Rect::new(a.0.min(b.0), a.1.min(b.1), (a.0 - b.0).abs(), (a.1 - b.1).abs())
    }
    fn c(&self) -> negs_rect_t {
        negs_rect_t { x: self.x, y: self.y, w: self.w, h: self.h }
    }
}

// MARK: The strip

/// A photograph's shape as its frames keep it: the one already `known`
/// (its header's, or its embedded preview's) unless a thumbnail's differs
/// by more than a thumbnail's rounding and a lens correction's trim, so
/// frames don't move as previews and developed thumbnails arrive. The
/// picture fills its frame, so what's left over is cut from its edges.
pub fn kept_ratio(known: Option<f64>, thumbnail: f64) -> f64 {
    unsafe { sys::negs_sheet_kept_ratio(flag(known.is_some()), known.unwrap_or(0.0), thumbnail) }
}

/// A frame's width: the picture's height at its photograph's proportions.
pub fn frame_width(ratio: Option<f64>) -> f64 {
    unsafe { sys::negs_sheet_frame_width(flag(ratio.is_some()), ratio.unwrap_or(0.0)) }
}

/// The strip's frames laid out: each one's left edge and width, edge to
/// edge but for the gaps. Worked out, never measured.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StripLayout {
    pub xs: Vec<f64>,
    pub widths: Vec<f64>,
    /// The whole row's width.
    pub width: f64,
}

impl StripLayout {
    pub fn new(widths: Vec<f64>) -> Self {
        let mut xs = Vec::with_capacity(widths.len());
        let width = unsafe {
            let width = sys::negs_sheet_strip_layout(widths.as_ptr(), widths.len(), xs.as_mut_ptr());
            xs.set_len(widths.len());
            width
        };
        Self { xs, widths, width }
    }

    pub fn count(&self) -> usize {
        self.xs.len()
    }

    pub fn middle(&self, index: usize) -> f64 {
        unsafe { sys::negs_sheet_strip_middle(self.xs[index], self.widths[index]) }
    }

    /// The frames' edges as the core reads them: as many widths as edges.
    fn parts(&self) -> (*const f64, *const f64, usize) {
        assert!(self.widths.len() >= self.xs.len(), "a strip layout has a width for every frame");
        (self.xs.as_ptr(), self.widths.as_ptr(), self.xs.len())
    }

    /// The frames reaching into `min_x…max_x`.
    pub fn indices(&self, min_x: f64, max_x: f64) -> std::ops::Range<usize> {
        let (xs, widths, n) = self.parts();
        let (mut start, mut end) = (0, 0);
        unsafe { sys::negs_sheet_strip_indices(xs, widths, n, min_x, max_x, &mut start, &mut end) };
        start..end
    }

    /// The frame under `x` (in the row), not the gaps.
    pub fn hit(&self, x: f64) -> Option<usize> {
        let (xs, widths, n) = self.parts();
        index(unsafe { sys::negs_sheet_strip_hit(xs, widths, n, x) })
    }

    /// The frame at `x`, or the nearest one; `None` without frames.
    pub fn index_at(&self, x: f64) -> Option<usize> {
        let (xs, widths, n) = self.parts();
        index(unsafe { sys::negs_sheet_strip_index_at(xs, widths, n, x) })
    }
}

/// The scroll offset that puts `middle` in the middle of a view `visible`
/// wide, as far as a row `content` wide goes (`StripSettling.target`).
pub fn centred(middle: f64, visible: f64, content: f64) -> f64 {
    unsafe { sys::negs_sheet_centred(middle, visible, content) }
}

/// The row was laid out anew (shapes read): the scroll offset that keeps
/// `index`'s middle where it was on screen (`PhotoBrowser.hold`).
pub fn hold(old: &StripLayout, new: &StripLayout, index: usize, scroll: f64, visible: f64) -> f64 {
    unsafe { sys::negs_sheet_hold(old.middle(index), new.middle(index), new.width, scroll, visible) }
}

/// The smoothstep glide back to the middle after a resize, 350 ms.
pub const GLIDE_MS: f64 = 350.0;
pub fn smoothstep(t: f64) -> f64 {
    unsafe { sys::negs_sheet_smoothstep(t) }
}

// MARK: The grid

pub const GRID_GAP: f64 = 6.0;
/// The grid's padding: room for a box on the first and last rows.
pub const GRID_TOP: f64 = 24.0;
pub const GRID_SIDE: f64 = 16.0;
pub const GRID_BOTTOM: f64 = 24.0;
/// The rows' chosen height (the foot's Size), and Ctrl+plus/minus's step.
pub const GRID_SIZES: (f64, f64) = (100.0, 360.0);
pub const GRID_DEFAULT: f64 = 176.0;
pub const GRID_SIZE_STEP: f64 = 40.0;
/// Cells are drawn this far above and below the grid's view.
pub const GRID_MARGIN: f64 = 300.0;
/// The sentence over the grid, and the foot under it.
pub const SENTENCE: f64 = 46.0;
pub const FOOT: f64 = STATUS;
/// The room kept round a box scrolled into view.
pub const BOX_MARGIN: f64 = 10.0;
/// How far the grid's yellow box reaches past its picture (`PickBox`'s
/// selection box sits 8 DIP out).
pub const SELECTION_OUTSET: f64 = 8.0;

pub fn clamp_size(size: f64) -> f64 {
    unsafe { sys::negs_sheet_clamp_size(size) }
}

/// negs_sheet_row_t's layout, so rows cross as they are.
#[repr(C)]
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub start: usize,
    pub end: usize,
    pub y: f64,
    pub height: f64,
}

impl Row {
    fn c(&self) -> sys::negs_sheet_row_t {
        sys::negs_sheet_row_t { start: self.start, end: self.end, y: self.y, height: self.height }
    }
}

/// The sheet's layout: thumbnails packed edge to edge in rows with gaps of
/// 6, each at its own proportions, every full row stretched or shrunk a
/// little to the sheet's width, the last row left short at the chosen
/// height.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetLayout {
    pub width: f64,
    pub frames: Vec<Rect>,
    pub rows: Vec<Row>,
    /// The whole grid's height, padding included.
    pub height: f64,
}

impl SheetLayout {
    /// `ratios`: each photograph's width ÷ height, in order.
    pub fn new(ratios: &[f64], width: f64, target: f64) -> Self {
        let n = ratios.len();
        let mut frames: Vec<Rect> = Vec::with_capacity(n);
        let mut rows: Vec<Row> = Vec::with_capacity(n);
        let mut row_count = 0;
        let height = unsafe {
            let height = sys::negs_sheet_layout(ratios.as_ptr(), n, width, target, frames.as_mut_ptr().cast(), rows.as_mut_ptr().cast(), &mut row_count);
            frames.set_len(n);
            rows.set_len(row_count);
            height
        };
        Self { width, frames, rows, height }
    }

    fn frames_c(&self) -> *const negs_rect_t {
        self.frames.as_ptr().cast()
    }

    fn rows_c(&self) -> *const sys::negs_sheet_row_t {
        self.rows.as_ptr().cast()
    }

    /// The row holding the `index`th photograph.
    pub fn row_of(&self, index: usize) -> Option<usize> {
        self::index(unsafe { sys::negs_sheet_row_of(self.rows_c(), self.rows.len(), self.frames.len(), index) })
    }

    /// The photograph `by` rows up (−) or down (+) from `index`: the one
    /// nearest it across. `None` past the first or last row.
    pub fn vertical(&self, index: usize, by: isize) -> Option<usize> {
        self::index(unsafe { sys::negs_sheet_vertical(self.frames_c(), self.frames.len(), self.rows_c(), self.rows.len(), index, by as i64) })
    }

    /// The cells `rect` touches.
    pub fn indices_in(&self, rect: &Rect) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.frames.len());
        unsafe {
            let n = sys::negs_sheet_indices_in(self.frames_c(), self.frames.len(), self.rows_c(), self.rows.len(), rect.c(), out.as_mut_ptr());
            out.set_len(n);
        }
        out
    }

    /// The photographs in the rows reaching into `min_y…max_y`.
    pub fn indices(&self, min_y: f64, max_y: f64) -> std::ops::Range<usize> {
        let (mut start, mut end) = (0, 0);
        unsafe { sys::negs_sheet_indices(self.rows_c(), self.rows.len(), min_y, max_y, &mut start, &mut end) };
        start..end
    }

    /// The cell under (`x`, `y`), not the gaps.
    pub fn hit(&self, x: f64, y: f64) -> Option<usize> {
        index(unsafe { sys::negs_sheet_hit(self.frames_c(), self.frames.len(), self.rows_c(), self.rows.len(), x, y) })
    }

    /// The photograph at a point, or else the nearest one.
    pub fn nearest(&self, x: f64, y: f64) -> Option<usize> {
        index(unsafe { sys::negs_sheet_nearest(self.frames_c(), self.frames.len(), self.rows_c(), self.rows.len(), x, y) })
    }

    /// The scroll offset that puts row `row` in the middle of a view
    /// `viewport` high, as far as the grid's ends let it.
    pub fn centre_row(&self, row: usize, viewport: f64) -> f64 {
        unsafe { sys::negs_sheet_centre_row(self.rows[row].c(), self.height, viewport) }
    }

    /// The least scroll from `scroll` that shows `index`'s yellow box whole,
    /// with `BOX_MARGIN` round it; up and down only.
    pub fn reveal(&self, index: usize, scroll: f64, viewport: f64) -> f64 {
        unsafe { sys::negs_sheet_reveal(self.frames[index].c(), self.height, scroll, viewport) }
    }
}

/// After the grid is laid out anew with the same photographs (shapes read,
/// a resize, a new Size), the scroll offset that keeps the photograph at
/// `index` where it was on screen (`SheetScroller.layoutChanged`): the
/// point `down` of the way down its picture stays `from_top` below the top.
pub fn keep_anchor(layout: &SheetLayout, index: usize, down: f64, from_top: f64, viewport: f64) -> f64 {
    unsafe { sys::negs_sheet_keep_anchor(layout.frames[index].c(), layout.height, down, from_top, viewport) }
}

// MARK: Thumbnails

/// The sizes thumbnails are read at (`PhotoThumbnails.Size`): the strip's
/// (frames 140 DIP tall, up to about 210 wide) and the grid's two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ThumbSize {
    Strip,
    Medium,
    Large,
}

impl ThumbSize {
    pub const ALL: [ThumbSize; 3] = [ThumbSize::Strip, ThumbSize::Medium, ThumbSize::Large];

    fn c(self) -> u32 {
        match self {
            ThumbSize::Strip => sys::NEGS_THUMB_STRIP,
            ThumbSize::Medium => sys::NEGS_THUMB_MEDIUM,
            ThumbSize::Large => sys::NEGS_THUMB_LARGE,
        }
    }

    fn of(c: u32) -> Self {
        match c {
            sys::NEGS_THUMB_STRIP => ThumbSize::Strip,
            sys::NEGS_THUMB_MEDIUM => ThumbSize::Medium,
            _ => ThumbSize::Large,
        }
    }

    /// The long side in pixels.
    pub fn pixels(self) -> u32 {
        unsafe { sys::negs_sheet_thumb_pixels(self.c()) }
    }

    /// The grid's, for pictures `points` on their long side at `scale`.
    pub fn grid(points: f64, scale: f64) -> Self {
        Self::of(unsafe { sys::negs_sheet_thumb_grid(points, scale) })
    }

    /// The grid's for a row height: landscapes are about 1.5 × it across.
    pub fn for_rows(size: f64, scale: f64) -> Self {
        Self::of(unsafe { sys::negs_sheet_thumb_for_rows(size, scale) })
    }

    /// Each size's memory cache: bytes and count (`PhotoThumbnails.caches`).
    pub fn cache_limits(self) -> (usize, usize) {
        let (mut bytes, mut count) = (0, 0);
        unsafe { sys::negs_sheet_thumb_cache_limits(self.c(), &mut bytes, &mut count) };
        (bytes, count)
    }
}

/// How far either side of the middle of the view thumbnails are read ahead.
pub const PREFETCH_REACH: usize = 32;
/// Photographs' shapes are read from their headers this many at a time
/// (`PhotoAspects.batch`).
pub const HEADER_BATCH: usize = 48;
/// The developed thumbnail's long side, border included, and the cache's size.
pub const EDITED_LONG_SIDE: u32 = 512;
pub const EDITED_LIMIT: u64 = 1 << 30;
/// Each photograph's last develop, screen-sized, kept as it's left.
pub const RENDER_LONG_SIDE: u32 = 2560;
pub const RENDERS_LIMIT: u64 = 2 << 30;

/// Where reading and developing go out from once those on screen are done:
/// the middle of the view (indices ascending), else the open photograph.
pub fn thumbnail_centre(on_screen: &[usize], open: Option<usize>) -> usize {
    unsafe { sys::negs_sheet_thumbnail_centre(on_screen.as_ptr(), on_screen.len(), flag(open.is_some()), open.unwrap_or(0)) }
}

/// `items` nearest `centre` first; of two as near, the one before.
pub fn outward<T>(items: Vec<(usize, T)>, centre: usize) -> Vec<T> {
    let indices: Vec<usize> = items.iter().map(|(i, _)| *i).collect();
    let mut order = vec![0usize; items.len()];
    unsafe { sys::negs_sheet_outward(indices.as_ptr(), indices.len(), centre, order.as_mut_ptr()) };
    let mut slots: Vec<Option<T>> = items.into_iter().map(|(_, t)| Some(t)).collect();
    order.into_iter().map(|p| slots[p].take().expect("each position once")).collect()
}

/// What to read ahead (`prefetchTargets`): the shown indices within
/// `PREFETCH_REACH` of the middle of the view (else the current one),
/// nearest first, but those on screen, which are read anyway.
pub fn prefetch_order(count: usize, on_screen: &[usize], current: Option<usize>) -> Vec<usize> {
    let mut out = [0usize; 2 * PREFETCH_REACH + 1];
    let n = unsafe {
        sys::negs_sheet_prefetch_order(count, on_screen.as_ptr(), on_screen.len(), flag(current.is_some()), current.unwrap_or(0), out.as_mut_ptr())
    };
    out[..n].to_vec()
}

/// One photograph as the developing pump sees it (negs_sheet_pump_item_t's
/// layout, so the items cross as they are).
#[repr(C)]
#[derive(Clone, Debug, PartialEq)]
pub struct PumpItem {
    /// Its look is known (its sidecar was read, and could be).
    pub look: bool,
    pub on_screen: bool,
    /// Open in the edit view: its thumbnail comes from its preview.
    pub open: bool,
    /// Being developed, already has a thumbnail, or failed with this look.
    pub done: bool,
}

/// The order thumbnails are developed in (`pumpThumbnails`): those on
/// screen first (in folder order), then the rest outward from the middle
/// of the view (or the open photograph); in Low Power those on screen only.
pub fn pump_order(items: &[PumpItem], low_power: bool) -> Vec<usize> {
    let mut out = Vec::with_capacity(items.len());
    unsafe {
        let n = sys::negs_sheet_pump_order(items.as_ptr().cast(), items.len(), flag(low_power), out.as_mut_ptr());
        out.set_len(n);
    }
    out
}

/// How many thumbnails develop at once, each in a helper of its own (about
/// 550 MB each): two with 8 cores and 16 GB, else one; one in Low Power.
pub fn helpers(cores: usize, memory: u64, low_power: bool) -> usize {
    unsafe { sys::negs_sheet_helpers(cores, memory, flag(low_power)) }
}

/// How many embedded previews are read at once: half the cores, 1…4.
pub fn readers(cores: usize) -> usize {
    unsafe { sys::negs_sheet_readers(cores) }
}

/// A cache bounded by bytes and count that lets the least recently used go.
pub struct Lru<K, V> {
    map: HashMap<K, (V, usize, u64)>,
    order: VecDeque<(K, u64)>,
    bytes: usize,
    tick: u64,
    max_bytes: usize,
    max_count: usize,
}

impl<K: Eq + Hash + Clone, V> Lru<K, V> {
    pub fn new(max_bytes: usize, max_count: usize) -> Self {
        Self { map: HashMap::new(), order: VecDeque::new(), bytes: 0, tick: 0, max_bytes, max_count }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// The value, now the most recently used.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.tick += 1;
        let tick = self.tick;
        let entry = self.map.get_mut(key)?;
        entry.2 = tick;
        self.order.push_back((key.clone(), tick));
        self.compact();
        self.map.get(key).map(|e| &e.0)
    }

    /// The value without making it recent.
    pub fn peek(&self, key: &K) -> Option<&V> {
        self.map.get(key).map(|e| &e.0)
    }

    pub fn insert(&mut self, key: K, value: V, cost: usize) {
        self.remove(&key);
        self.tick += 1;
        self.bytes += cost;
        self.map.insert(key.clone(), (value, cost, self.tick));
        self.order.push_back((key, self.tick));
        while self.bytes > self.max_bytes || self.map.len() > self.max_count {
            let Some((old, tick)) = self.order.pop_front() else { break };
            if self.map.get(&old).is_some_and(|e| e.2 == tick) {
                let (_, cost, _) = self.map.remove(&old).expect("present");
                self.bytes -= cost;
            }
        }
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let (value, cost, _) = self.map.remove(key)?;
        self.bytes -= cost;
        Some(value)
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) {
        let bytes = &mut self.bytes;
        self.map.retain(|k, (v, cost, _)| {
            let kept = keep(k, v);
            if !kept {
                *bytes -= *cost;
            }
            kept
        });
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
        self.bytes = 0;
    }

    /// Stale order entries (from `get`) are dropped once they outnumber the
    /// entries, so the queue stays bounded.
    fn compact(&mut self) {
        if self.order.len() > 2 * self.map.len() + 64 {
            let map = &self.map;
            self.order.retain(|(k, t)| map.get(k).is_some_and(|e| e.2 == *t));
        }
    }
}

// MARK: The sentence and the foot (SheetWords)

/// A piece of the sentence over the grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    /// Words in the sentence's italic.
    Words(String),
    /// An underlined word that opens the filter menu; `on` while filtered
    /// (the pencil under it).
    Filter { word: String, on: bool },
    /// The × that clears the filter.
    Clear,
}

fn filter_c(filter: &Filter) -> sys::negs_sheet_filter_t {
    let (rating, stars) = match filter.rating {
        RatingFilter::Any => (0, 0),
        RatingFilter::AtLeast(n) => (1, n),
        RatingFilter::Unrated => (2, 0),
    };
    let flags = filter.flags.iter().fold(0u8, |bits, f| {
        bits | match f {
            FlagFilter::Pick => 1,
            FlagFilter::Unflagged => 2,
            FlagFilter::Reject => 4,
        }
    });
    let edits = match filter.edits {
        EditsFilter::Any => 0,
        EditsFilter::Edited => 1,
        EditsFilter::Unedited => 2,
    };
    sys::negs_sheet_filter_t { rating, stars, flags, edits }
}

/// "Showing *all* 12 photographs", or when filtered "Showing 4 of 12 ·
/// rated *★★★ or more*, *picks*, *edited* ×".
pub fn sentence(filter: &Filter, shown: usize, total: usize) -> Vec<Part> {
    let f = filter_c(filter);
    let mut parts = [sys::negs_sheet_part_t::default(); sys::NEGS_SHEET_MAX_PARTS];
    let mut text = vec![0u8; 256];
    let mut len = 0;
    let mut count = unsafe { sys::negs_sheet_sentence(&f, shown, total, parts.as_mut_ptr(), parts.len(), text.as_mut_ptr(), text.len(), &mut len) };
    if len > text.len() {
        text = vec![0u8; len];
        count = unsafe { sys::negs_sheet_sentence(&f, shown, total, parts.as_mut_ptr(), parts.len(), text.as_mut_ptr(), text.len(), &mut len) };
    }
    parts[..count.min(parts.len())]
        .iter()
        .map(|p| {
            let words = String::from_utf8(text[p.offset..p.offset + p.len].to_vec()).expect("the core writes UTF-8");
            match p.kind {
                sys::NEGS_SHEET_PART_WORDS => Part::Words(words),
                sys::NEGS_SHEET_PART_FILTER => Part::Filter { word: words, on: p.on != 0 },
                _ => Part::Clear,
            }
        })
        .collect()
}

/// The foot's count: "12 photographs · 1 selected", or "4 of 12 photographs".
pub fn foot(shown: usize, total: usize, selected: usize, filtered: bool) -> String {
    sys::text(|out, cap| unsafe { sys::negs_sheet_foot(shown, total, selected, flag(filtered), out, cap) })
}

/// The foot's Size slider: its value as a share of its track, and back.
pub fn size_fraction(size: f64) -> f64 {
    unsafe { sys::negs_sheet_size_fraction(size) }
}

pub fn size_at(fraction: f64) -> f64 {
    unsafe { sys::negs_sheet_size_at(fraction) }
}

// MARK: The grid's fade

/// The grid fades in over the photograph as it rises a little, and out
/// again. Windows only: the Mac's grid flies out of the strip
/// (`SheetFlight`), which here, painted a frame at a time at 59 Hz, felt
/// slow (Noah, 2026-11-20); a fade the compositor runs doesn't.
pub const FADE_MS: f64 = 150.0;
/// How far below its place the fading sheet starts (DIPs).
pub const FADE_RISE: f64 = 14.0;

/// The sheet `along` ms into its fade (closing plays it backwards): its
/// opacity, and how far below its place it is (DIPs), settling on
/// `cubic-bezier(.22, .8, .24, 1)`.
pub fn sheet_look(along: f64) -> (f64, f64) {
    let (mut opacity, mut rise) = (0.0, 0.0);
    unsafe { sys::negs_sheet_look(along, &mut opacity, &mut rise) };
    (opacity, rise)
}

/// A CSS cubic Bézier timing function at `t` (0…1).
pub fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, t: f64) -> f64 {
    unsafe { sys::negs_sheet_cubic_bezier(x1, y1, x2, y2, t) }
}

/// `f` over `0…length` as `pieces` cubics, for the compositor's
/// animations: each piece's start and its coefficients in time since it
/// (`c0 + c1·u + c2·u² + c3·u³`), meeting `f` and its slope at both ends.
/// The core samples `f` through a callback, on this thread; `f` must not
/// panic (a panic can't unwind through the core: the process aborts).
pub fn cubic_pieces(f: impl Fn(f64) -> f64, length: f64, pieces: usize) -> Vec<[f64; 5]> {
    unsafe extern "C" fn call<F: Fn(f64) -> f64>(user: *mut c_void, t: f64) -> f64 {
        let f = unsafe { &*(user as *const F) };
        f(t)
    }
    fn run<F: Fn(f64) -> f64>(f: &F, length: f64, pieces: usize) -> Vec<[f64; 5]> {
        let mut out = vec![[0.0; 5]; pieces.max(1)];
        unsafe { sys::negs_sheet_cubic_pieces(call::<F>, f as *const F as *mut c_void, length, pieces, out.as_mut_ptr().cast()) };
        out
    }
    run(&f, length, pieces)
}

// MARK: The grid's scrolling

/// A wheel's notch glides the grid there, quickly, then easing off (the
/// Mac's scroll view coasts after a flick); notches add up. A precision
/// touchpad's small steps, which Windows already coasts, go straight.
pub const WHEEL_STEP: f64 = 100.0;
pub const WHEEL_TAU_MS: f64 = 55.0;

/// Where a wheel's glide from `at` towards `to` is `dt_ms` later; `to`
/// once within half a DIP.
pub fn wheel_glide(at: f64, to: f64, dt_ms: f64) -> f64 {
    unsafe { sys::negs_sheet_wheel_glide(at, to, dt_ms) }
}

/// The overlay scroll bar at the grid's right edge (`.scrollIndicators(
/// .automatic)`, macOS's overlay scroller): a rounded knob 7 wide, 11
/// under the pointer, 2 in from the edge, at least 24 long; shown while
/// scrolling and for a second after, then fading in 0.3 s.
pub const BAR_WIDTH: f64 = 7.0;
pub const BAR_WIDE: f64 = 11.0;
pub const BAR_INSET: f64 = 2.0;
pub const BAR_MIN: f64 = 24.0;
/// The strip at the right edge where the pointer finds the bar.
pub const BAR_REACH: f64 = 16.0;
pub const BAR_SHOWN_MS: f64 = 1000.0;
pub const BAR_FADE_MS: f64 = 300.0;

/// The knob's top and length in a track `viewport` long, for a sheet
/// `content` high scrolled to `scroll`; None when it all fits.
pub fn bar_knob(scroll: f64, content: f64, viewport: f64) -> Option<(f64, f64)> {
    let (mut top, mut length) = (0.0, 0.0);
    let shown = unsafe { sys::negs_sheet_bar_knob(scroll, content, viewport, &mut top, &mut length) };
    (shown != 0).then_some((top, length))
}

/// The scroll that puts the knob's top at `top` (dragging it).
pub fn bar_scroll(top: f64, content: f64, viewport: f64) -> f64 {
    unsafe { sys::negs_sheet_bar_scroll(top, content, viewport) }
}

/// The bar's opacity `since_ms` after the grid last scrolled.
pub fn bar_opacity(since_ms: f64) -> f64 {
    unsafe { sys::negs_sheet_bar_opacity(since_ms) }
}

#[cfg(test)]
mod windows_tests;

#[cfg(test)]
mod core_tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn the_constants_are_the_cores() {
        let mut c = sys::negs_sheet_constants_t::default();
        unsafe { sys::negs_sheet_constants(&mut c) };
        let ours = [
            STRIP, STRIP_TOP, STRIP_REBATE, STRIP_GAP, PICTURE, STATUS, BAND, STRIP_DIM, STRIP_HOVER, STRIP_PRESSED, PENCIL_WIDTH, PENCIL_DROP,
            FALLBACK_RATIO, STRIP_MARGIN, LEADER_WIDTH, LEADER_HEIGHT, LEADER_RAISED, LEADER_SLANT, GLIDE_MS, GRID_GAP, GRID_TOP, GRID_SIDE,
            GRID_BOTTOM, GRID_SIZES.0, GRID_SIZES.1, GRID_DEFAULT, GRID_SIZE_STEP, GRID_MARGIN, SENTENCE, FOOT, BOX_MARGIN, SELECTION_OUTSET,
            FADE_MS, FADE_RISE, WHEEL_STEP, WHEEL_TAU_MS, BAR_WIDTH, BAR_WIDE, BAR_INSET, BAR_MIN, BAR_REACH, BAR_SHOWN_MS, BAR_FADE_MS,
        ];
        let core = [
            c.strip, c.strip_top, c.strip_rebate, c.strip_gap, c.picture, c.status, c.band, c.strip_dim, c.strip_hover, c.strip_pressed,
            c.pencil_width, c.pencil_drop, c.fallback_ratio, c.strip_margin, c.leader_width, c.leader_height, c.leader_raised, c.leader_slant,
            c.glide_ms, c.grid_gap, c.grid_top, c.grid_side, c.grid_bottom, c.grid_size_min, c.grid_size_max, c.grid_default, c.grid_size_step,
            c.grid_margin, c.sentence, c.foot, c.box_margin, c.selection_outset, c.fade_ms, c.fade_rise, c.wheel_step, c.wheel_tau_ms,
            c.bar_width, c.bar_wide, c.bar_inset, c.bar_min, c.bar_reach, c.bar_shown_ms, c.bar_fade_ms,
        ];
        for (i, (a, b)) in ours.iter().zip(core).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "constant {i}");
        }
        assert_eq!((c.prefetch_reach as usize, c.header_batch as usize), (PREFETCH_REACH, HEADER_BATCH));
        assert_eq!((c.edited_long_side, c.render_long_side), (EDITED_LONG_SIDE, RENDER_LONG_SIDE));
        assert_eq!((c.edited_limit, c.renders_limit), (EDITED_LIMIT, RENDERS_LIMIT));
    }

    #[test]
    fn rects_rows_and_pump_items_cross_as_they_are() {
        use negs_core_sys::sheet::{negs_sheet_pump_item_t as P, negs_sheet_row_t as R};
        assert_eq!((size_of::<Rect>(), size_of::<negs_rect_t>()), (32, 32));
        assert_eq!(offset_of!(Rect, h), offset_of!(negs_rect_t, h));
        assert_eq!(size_of::<Row>(), size_of::<R>());
        assert_eq!((offset_of!(Row, end), offset_of!(Row, y), offset_of!(Row, height)), (offset_of!(R, end), offset_of!(R, y), offset_of!(R, height)));
        assert_eq!(size_of::<PumpItem>(), size_of::<P>());
        assert_eq!(
            (offset_of!(PumpItem, on_screen), offset_of!(PumpItem, open), offset_of!(PumpItem, done)),
            (offset_of!(P, on_screen), offset_of!(P, open), offset_of!(P, done))
        );
    }
}
