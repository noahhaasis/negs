//! The film strip's and the grid's rules, with no Win32 in them, so their
//! tests run on any OS (`windows/sheet-rules/`): where every frame and
//! thumbnail goes (`StripLayout`, `SheetLayout` in `ScrollWatch.swift` and
//! `Selection.swift`), the order thumbnails are read and developed in
//! (`EditorModel.thumbnailCentre`, `outward`, `prefetchTargets`,
//! `pumpThumbnails`), how many at once, the bounded memory cache, the
//! grid's sentence and foot (`SheetWords`), and the fold flight's timing
//! (`SheetFlight.swift`). Lengths are DIPs, the Mac's points.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

use negs_library::Filter;

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
    if in_strip { if current { 0.5 } else { 0.22 } } else { 0.4 }
}

/// How the strip shows a frame: the current and selected ones whole, the
/// rest dimmed, lifted under the pointer, a reject greyed further.
pub fn strip_opacity(current: bool, selected: bool, rejected: bool, hovered: bool, pressed: bool) -> f64 {
    if rejected {
        return reject_opacity(true, current || selected);
    }
    if current || selected {
        1.0
    } else if pressed {
        STRIP_PRESSED
    } else if hovered {
        STRIP_HOVER
    } else {
        STRIP_DIM
    }
}

// MARK: Rectangles

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
    #[cfg(test)]
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
    /// `self` → `to` by `t` (0…1), each edge on its own.
    pub fn lerp(&self, to: &Rect, t: f64) -> Rect {
        let l = |a: f64, b: f64| a + (b - a) * t;
        Rect::new(l(self.x, to.x), l(self.y, to.y), l(self.w, to.w), l(self.h, to.h))
    }
}

// MARK: The strip

/// A frame's width: the picture's height at its photograph's proportions.
pub fn frame_width(ratio: Option<f64>) -> f64 {
    (PICTURE * ratio.unwrap_or(FALLBACK_RATIO).clamp(0.1, 10.0)).round()
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
        let mut x = 0.0;
        for w in &widths {
            xs.push(x);
            x += w + STRIP_GAP;
        }
        let width = widths.iter().sum::<f64>() + STRIP_GAP * widths.len().saturating_sub(1) as f64;
        Self { xs, widths, width }
    }

    pub fn count(&self) -> usize {
        self.xs.len()
    }

    pub fn middle(&self, index: usize) -> f64 {
        self.xs[index] + self.widths[index] / 2.0
    }

    /// The first index for which `test` holds, `count` if none.
    fn partition(&self, test: impl Fn(usize) -> bool) -> usize {
        let (mut low, mut high) = (0, self.count());
        while low < high {
            let middle = (low + high) / 2;
            if test(middle) { high = middle } else { low = middle + 1 }
        }
        low
    }

    /// The frames reaching into `min_x…max_x`.
    pub fn indices(&self, min_x: f64, max_x: f64) -> std::ops::Range<usize> {
        let start = self.partition(|i| self.xs[i] + self.widths[i] >= min_x);
        let end = self.partition(|i| self.xs[i] > max_x);
        start..end.max(start)
    }

    /// The frame under `x` (in the row), not the gaps.
    pub fn hit(&self, x: f64) -> Option<usize> {
        let index = self.partition(|i| self.xs[i] + self.widths[i] > x);
        (index < self.count() && x >= self.xs[index]).then_some(index)
    }

    /// The frame at `x`, or the nearest one; `None` without frames.
    pub fn index_at(&self, x: f64) -> Option<usize> {
        (self.count() > 0).then(|| self.partition(|i| self.xs[i] + self.widths[i] >= x).min(self.count() - 1))
    }
}

/// The scroll offset that puts `middle` in the middle of a view `visible`
/// wide, as far as a row `content` wide goes (`StripSettling.target`).
pub fn centred(middle: f64, visible: f64, content: f64) -> f64 {
    (middle - visible / 2.0).max(0.0).min((content - visible).max(0.0))
}

/// The row was laid out anew (shapes read): the scroll offset that keeps
/// `index`'s middle where it was on screen (`PhotoBrowser.hold`).
pub fn hold(old: &StripLayout, new: &StripLayout, index: usize, scroll: f64, visible: f64) -> f64 {
    let x = scroll + new.middle(index) - old.middle(index);
    x.max(0.0).min((new.width - visible).max(0.0))
}

/// The smoothstep glide back to the middle after a resize, 350 ms.
pub const GLIDE_MS: f64 = 350.0;
pub fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
    size.clamp(GRID_SIZES.0, GRID_SIZES.1).round()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub start: usize,
    pub end: usize,
    pub y: f64,
    pub height: f64,
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
        let usable = (width - 2.0 * GRID_SIDE).max(1.0);
        let target = target.max(1.0);
        let mut frames = Vec::with_capacity(ratios.len());
        let mut rows = Vec::new();
        let (mut y, mut start) = (GRID_TOP, 0);
        let ratio = |i: usize| ratios[i].clamp(0.1, 10.0);
        while start < ratios.len() {
            let (mut end, mut sum, mut full) = (start, 0.0, false);
            while end < ratios.len() {
                let next = sum + ratio(end);
                let gaps = GRID_GAP * (end - start) as f64;
                if next * target + gaps < usable {
                    sum = next;
                    end += 1;
                    continue;
                }
                full = true;
                // Take this one too if that leaves the row nearer the
                // chosen height than stopping short of it does.
                if end == start || ((usable - gaps) / next / target).ln().abs() <= ((usable - gaps + GRID_GAP) / sum / target).ln().abs() {
                    sum = next;
                    end += 1;
                }
                break;
            }
            let count = end - start;
            let height = if full { ((usable - GRID_GAP * (count - 1) as f64) / sum).round() } else { target.round() };
            let mut x = GRID_SIDE;
            for index in start..end {
                // Whole DIPs; a full row's last cell ends flush at the right.
                let w = if full && index == end - 1 { GRID_SIDE + usable - x } else { (ratio(index) * height).round().max(1.0) };
                frames.push(Rect::new(x, y, w, height));
                x += w + GRID_GAP;
            }
            rows.push(Row { start, end, y, height });
            y += height + GRID_GAP;
            start = end;
        }
        let height = if rows.is_empty() { GRID_TOP + GRID_BOTTOM } else { y - GRID_GAP + GRID_BOTTOM };
        Self { width, frames, rows, height }
    }

    /// The row holding the `index`th photograph.
    pub fn row_of(&self, index: usize) -> Option<usize> {
        if index >= self.frames.len() {
            return None;
        }
        Some(self.rows.partition_point(|r| r.start <= index) - 1)
    }

    /// The photograph `by` rows up (−) or down (+) from `index`: the one
    /// nearest it across. `None` past the first or last row.
    pub fn vertical(&self, index: usize, by: isize) -> Option<usize> {
        let row = self.row_of(index)? as isize + by;
        let row = self.rows.get(usize::try_from(row).ok()?)?;
        let x = self.frames[index].mid_x();
        (row.start..row.end).min_by(|&a, &b| (self.frames[a].mid_x() - x).abs().total_cmp(&(self.frames[b].mid_x() - x).abs()))
    }

    /// The cells `rect` touches.
    pub fn indices_in(&self, rect: &Rect) -> Vec<usize> {
        self.rows
            .iter()
            .filter(|r| r.y <= rect.max_y() && r.y + r.height >= rect.y)
            .flat_map(|r| (r.start..r.end).filter(|&i| self.frames[i].intersects(rect)))
            .collect()
    }

    /// The photographs in the rows reaching into `min_y…max_y`.
    pub fn indices(&self, min_y: f64, max_y: f64) -> std::ops::Range<usize> {
        let first = self.rows.partition_point(|r| r.y + r.height < min_y);
        let last = self.rows.partition_point(|r| r.y <= max_y);
        if first >= last {
            return 0..0;
        }
        self.rows[first].start..self.rows[last - 1].end
    }

    /// The cell under (`x`, `y`), not the gaps.
    pub fn hit(&self, x: f64, y: f64) -> Option<usize> {
        let row = self.rows.iter().find(|r| y >= r.y && y < r.y + r.height)?;
        (row.start..row.end).find(|&i| self.frames[i].contains(x, y))
    }

    /// The photograph at a point, or else the nearest one.
    pub fn nearest(&self, x: f64, y: f64) -> Option<usize> {
        let distance = |low: f64, high: f64, v: f64| (low - v).max(v - high).max(0.0);
        let row = self.rows.iter().min_by(|a, b| distance(a.y, a.y + a.height, y).total_cmp(&distance(b.y, b.y + b.height, y)))?;
        (row.start..row.end).min_by(|&a, &b| {
            let (fa, fb) = (&self.frames[a], &self.frames[b]);
            distance(fa.x, fa.max_x(), x).total_cmp(&distance(fb.x, fb.max_x(), x))
        })
    }

    /// The scroll offset that puts row `row` in the middle of a view
    /// `viewport` high, as far as the grid's ends let it.
    pub fn centre_row(&self, row: usize, viewport: f64) -> f64 {
        let r = &self.rows[row];
        (r.y + r.height / 2.0 - viewport / 2.0).max(0.0).min((self.height - viewport).max(0.0))
    }

    /// The least scroll from `scroll` that shows `index`'s yellow box whole,
    /// with `BOX_MARGIN` round it; up and down only.
    pub fn reveal(&self, index: usize, scroll: f64, viewport: f64) -> f64 {
        let f = &self.frames[index];
        let (top, bottom) = (f.y - SELECTION_OUTSET - BOX_MARGIN, f.max_y() + SELECTION_OUTSET + BOX_MARGIN);
        let max = (self.height - viewport).max(0.0);
        let s = if top < scroll { top } else if bottom > scroll + viewport { bottom - viewport } else { scroll };
        s.max(0.0).min(max)
    }
}

/// After the grid is laid out anew with the same photographs (shapes read,
/// a resize, a new Size), the scroll offset that keeps the photograph at
/// `index` where it was on screen (`SheetScroller.layoutChanged`): the
/// point `down` of the way down its picture stays `from_top` below the top.
pub fn keep_anchor(layout: &SheetLayout, index: usize, down: f64, from_top: f64, viewport: f64) -> f64 {
    let f = &layout.frames[index];
    (f.y + down * f.h - from_top).max(0.0).min((layout.height - viewport).max(0.0))
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

    /// The long side in pixels.
    pub fn pixels(self) -> u32 {
        match self {
            ThumbSize::Strip => 432,
            ThumbSize::Medium => 384,
            ThumbSize::Large => 768,
        }
    }

    /// The grid's, for pictures `points` on their long side at `scale`.
    pub fn grid(points: f64, scale: f64) -> Self {
        if points * scale <= 384.0 { ThumbSize::Medium } else { ThumbSize::Large }
    }

    /// The grid's for a row height: landscapes are about 1.5 × it across.
    pub fn for_rows(size: f64, scale: f64) -> Self {
        Self::grid(size * 1.5, scale)
    }

    /// Each size's memory cache: bytes and count (`PhotoThumbnails.caches`).
    pub fn cache_limits(self) -> (usize, usize) {
        match self {
            ThumbSize::Strip => (48 << 20, 120),
            ThumbSize::Medium => (48 << 20, 400),
            ThumbSize::Large => (96 << 20, 200),
        }
    }
}

/// How far either side of the middle of the view thumbnails are read ahead.
pub const PREFETCH_REACH: usize = 32;
/// The developed thumbnail's long side, border included, and the cache's size.
pub const EDITED_LONG_SIDE: u32 = 512;
pub const EDITED_LIMIT: u64 = 1 << 30;
/// Each photograph's last develop, screen-sized, kept as it's left.
pub const RENDER_LONG_SIDE: u32 = 2560;
pub const RENDERS_LIMIT: u64 = 2 << 30;

/// Where reading and developing go out from once those on screen are done:
/// the middle of the view (indices ascending), else the open photograph.
pub fn thumbnail_centre(on_screen: &[usize], open: Option<usize>) -> usize {
    if on_screen.is_empty() { open.unwrap_or(0) } else { on_screen[on_screen.len() / 2] }
}

/// `items` nearest `centre` first; of two as near, the one before.
pub fn outward<T>(mut items: Vec<(usize, T)>, centre: usize) -> Vec<T> {
    items.sort_by_key(|(i, _)| (i.abs_diff(centre), *i));
    items.into_iter().map(|(_, t)| t).collect()
}

/// What to read ahead (`prefetchTargets`): the shown indices within
/// `PREFETCH_REACH` of the middle of the view (else the current one),
/// nearest first, but those on screen, which are read anyway.
pub fn prefetch_order(count: usize, on_screen: &[usize], current: Option<usize>) -> Vec<usize> {
    if count == 0 {
        return vec![];
    }
    let centre = thumbnail_centre(on_screen, current).min(count - 1);
    let (low, high) = (centre.saturating_sub(PREFETCH_REACH), (centre + PREFETCH_REACH).min(count - 1));
    outward((low..=high).filter(|i| !on_screen.contains(i)).map(|i| (i, i)).collect(), centre)
}

/// One photograph as the developing pump sees it.
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
    let on_screen: Vec<usize> = items.iter().enumerate().filter(|(_, i)| i.on_screen && !i.open).map(|(n, _)| n).collect();
    let centre = thumbnail_centre(&on_screen, items.iter().position(|i| i.open));
    let wanted = |i: &PumpItem| i.look && !i.open && !i.done;
    let mut order: Vec<usize> = items.iter().enumerate().filter(|(_, i)| i.on_screen && wanted(i)).map(|(n, _)| n).collect();
    if !low_power {
        let rest = items.iter().enumerate().filter(|(_, i)| !i.on_screen && wanted(i)).map(|(n, _)| (n, n)).collect();
        order.extend(outward(rest, centre));
    }
    order
}

/// How many thumbnails develop at once, each in a helper of its own (about
/// 550 MB each): two with 8 cores and 16 GB, else one; one in Low Power.
pub fn helpers(cores: usize, memory: u64, low_power: bool) -> usize {
    if !low_power && cores >= 8 && memory >= 16 << 30 { 2 } else { 1 }
}

/// How many embedded previews are read at once: half the cores, 1…4.
pub fn readers(cores: usize) -> usize {
    (cores / 2).clamp(1, 4)
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

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[cfg(test)]
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

    #[cfg(test)]
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

/// "Showing *all* 12 photographs", or when filtered "Showing 4 of 12 ·
/// rated *★★★ or more*, *picks*, *edited* ×".
pub fn sentence(filter: &Filter, shown: usize, total: usize) -> Vec<Part> {
    if !filter.active() {
        return vec![
            Part::Words("Showing ".into()),
            Part::Filter { word: "all".into(), on: false },
            Part::Words(format!(" {}", negs_library::filter::photographs(total))),
        ];
    }
    let mut parts = vec![Part::Words(format!("Showing {shown} of {total} · "))];
    for (index, word) in filter.words().into_iter().enumerate() {
        let lead = if index > 0 { format!(", {}", word.lead) } else { word.lead.clone() };
        if !lead.is_empty() {
            parts.push(Part::Words(lead));
        }
        parts.push(Part::Filter { word: word.word, on: true });
    }
    parts.push(Part::Clear);
    parts
}

/// The foot's count: "12 photographs · 1 selected", or "4 of 12 photographs".
pub fn foot(shown: usize, total: usize, selected: usize, filtered: bool) -> String {
    negs_library::filter::count_words(shown, total, selected, filtered)
}

/// The foot's Size slider: its value as a share of its track, and back.
pub fn size_fraction(size: f64) -> f64 {
    ((size - GRID_SIZES.0) / (GRID_SIZES.1 - GRID_SIZES.0)).clamp(0.0, 1.0)
}

pub fn size_at(fraction: f64) -> f64 {
    clamp_size(GRID_SIZES.0 + fraction.clamp(0.0, 1.0) * (GRID_SIZES.1 - GRID_SIZES.0))
}

// MARK: The fold flight (SheetFlight)

/// Each flyer's flight; each step from the current photograph starts 8 ms
/// later, up to 12 steps; the grid's boxes fade in from 35 % of a flight;
/// a fold's last 110 ms fade the sheet off the strip. Shorter than the
/// Mac's (480, 16, 180: 672 ms in all), 396 ms in all, at Noah's asking.
pub const FLIGHT_MS: f64 = 300.0;
pub const STAGGER_MS: f64 = 8.0;
pub const STAGGER_STEPS: usize = 12;
pub const BOXES_FROM: f64 = 0.35;
pub const FOLD_FADE_MS: f64 = 110.0;
/// How far past the window's top the sheet's growing edge is aimed.
pub const MASK_REACH: f64 = 0.25;

/// `cubic-bezier(.22, .8, .24, 1)` at `t`.
pub fn flight_curve(t: f64) -> f64 {
    cubic_bezier(0.22, 0.8, 0.24, 1.0, t)
}

/// A CSS cubic Bézier timing function at `t` (0…1).
pub fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t == 0.0 || t == 1.0 {
        return t;
    }
    let (cx, bx) = (3.0 * x1, 3.0 * (x2 - x1) - 3.0 * x1);
    let ax = 1.0 - cx - bx;
    let (cy, by) = (3.0 * y1, 3.0 * (y2 - y1) - 3.0 * y1);
    let ay = 1.0 - cy - by;
    let x = |s: f64| ((ax * s + bx) * s + cx) * s;
    let dx = |s: f64| (3.0 * ax * s + 2.0 * bx) * s + cx;
    // Newton, then bisection where the slope is too flat.
    let mut s = t;
    for _ in 0..8 {
        let d = dx(s);
        if d.abs() < 1e-6 {
            break;
        }
        let next = s - (x(s) - t) / d;
        if (next - s).abs() < 1e-7 {
            s = next;
            break;
        }
        s = next;
    }
    if !(0.0..=1.0).contains(&s) || (x(s) - t).abs() > 1e-5 {
        let (mut low, mut high) = (0.0, 1.0);
        s = t;
        for _ in 0..40 {
            if x(s) < t { low = s } else { high = s }
            s = (low + high) / 2.0;
        }
    }
    ((ay * s + by) * s + cy) * s
}

/// A flyer's delay: 8 ms a step from the current photograph, up to 12.
pub fn flight_delay(index: usize, current: Option<usize>) -> f64 {
    let steps = current.map_or(0, |c| index.abs_diff(c)).min(STAGGER_STEPS);
    steps as f64 * STAGGER_MS
}

/// Where a flyer is `elapsed` ms into the flight: `from` → `to`, after its
/// delay, on the curve. Folding plays it backwards.
pub fn flyer_at(from: &Rect, to: &Rect, elapsed: f64, delay: f64) -> Rect {
    let t = ((elapsed - delay) / FLIGHT_MS).clamp(0.0, 1.0);
    from.lerp(to, flight_curve(t))
}

/// The whole flight's length with `flyers` staggered as `flight_delay` does.
pub fn flight_length(longest_delay: f64) -> f64 {
    longest_delay + FLIGHT_MS
}

/// The sheet's growing edge: from the strip's `band` towards `window`,
/// aimed `MASK_REACH` past its top so the sentence isn't left for the end,
/// and never past the window.
pub fn mask_at(band: &Rect, window: &Rect, elapsed: f64) -> Rect {
    let reach = window.h * MASK_REACH;
    let aimed = Rect::new(window.x, window.y - reach, window.w, window.h + reach);
    let t = flight_curve((elapsed / FLIGHT_MS).clamp(0.0, 1.0));
    let r = band.lerp(&aimed, t);
    let top = r.y.max(window.y);
    Rect::new(r.x.max(window.x), top, r.w.min(window.w), (r.max_y() - top).max(0.0))
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
    let next = at + (to - at) * (1.0 - (-dt_ms.max(0.0) / WHEEL_TAU_MS).exp());
    if (to - next).abs() < 0.5 { to } else { next }
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
    let track = viewport - 2.0 * BAR_INSET;
    if content <= viewport + 0.5 || track <= BAR_MIN {
        return None;
    }
    let length = (track * viewport / content).max(BAR_MIN).min(track);
    let room = (content - viewport).max(1.0);
    let top = BAR_INSET + (track - length) * (scroll / room).clamp(0.0, 1.0);
    Some((top, length))
}

/// The scroll that puts the knob's top at `top` (dragging it).
pub fn bar_scroll(top: f64, content: f64, viewport: f64) -> f64 {
    let Some((_, length)) = bar_knob(0.0, content, viewport) else { return 0.0 };
    let track = viewport - 2.0 * BAR_INSET - length;
    let room = (content - viewport).max(0.0);
    if track <= 0.0 { 0.0 } else { ((top - BAR_INSET) / track).clamp(0.0, 1.0) * room }
}

/// The bar's opacity `since_ms` after the grid last scrolled.
pub fn bar_opacity(since_ms: f64) -> f64 {
    if since_ms <= BAR_SHOWN_MS { 1.0 } else { (1.0 - (since_ms - BAR_SHOWN_MS) / BAR_FADE_MS).clamp(0.0, 1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use negs_library::{EditsFilter, FlagFilter, RatingFilter};

    #[test]
    fn a_wheel_glides_quickly_then_eases_off() {
        let first = wheel_glide(0.0, 100.0, 16.0);
        assert!(first > 20.0 && first < 40.0, "{first}");
        let second = wheel_glide(first, 100.0, 16.0) - first;
        assert!(second < first, "it eases off");
        let mut at = 0.0;
        let mut frames = 0;
        while at != 100.0 {
            at = wheel_glide(at, 100.0, 16.0);
            frames += 1;
        }
        assert!((10..=25).contains(&frames), "lands in about a quarter second: {frames} frames");
        assert_eq!(wheel_glide(50.0, 50.0, 16.0), 50.0);
        assert_eq!(wheel_glide(100.0, 0.0, 16.0) < 100.0, true);
    }

    #[test]
    fn the_scroll_bar_knob_follows_the_scroll() {
        assert_eq!(bar_knob(0.0, 500.0, 600.0), None, "nothing to scroll");
        let (top, length) = bar_knob(0.0, 2400.0, 600.0).unwrap();
        assert_eq!(top, BAR_INSET);
        assert_eq!(length, (600.0 - 2.0 * BAR_INSET) / 4.0);
        let (bottom, _) = bar_knob(1800.0, 2400.0, 600.0).unwrap();
        assert_eq!(bottom + length, 600.0 - BAR_INSET);
        let (_, tiny) = bar_knob(0.0, 600_000.0, 600.0).unwrap();
        assert_eq!(tiny, BAR_MIN);
        // Dragging the knob back to where it shows a scroll gives that scroll.
        let (mid, _) = bar_knob(900.0, 2400.0, 600.0).unwrap();
        assert!((bar_scroll(mid, 2400.0, 600.0) - 900.0).abs() < 1e-9);
        assert_eq!(bar_scroll(-50.0, 2400.0, 600.0), 0.0);
        assert_eq!(bar_scroll(5000.0, 2400.0, 600.0), 1800.0);
    }

    #[test]
    fn the_scroll_bar_shows_a_second_then_fades() {
        assert_eq!(bar_opacity(0.0), 1.0);
        assert_eq!(bar_opacity(BAR_SHOWN_MS), 1.0);
        assert!((bar_opacity(BAR_SHOWN_MS + BAR_FADE_MS / 2.0) - 0.5).abs() < 1e-9);
        assert_eq!(bar_opacity(BAR_SHOWN_MS + BAR_FADE_MS), 0.0);
        assert_eq!(bar_opacity(1e9), 0.0);
    }

    #[test]
    fn strip_band_is_the_trays_height() {
        assert_eq!(PICTURE, 140.0);
        assert_eq!(BAND, 186.0);
    }

    #[test]
    fn strip_frames_take_their_proportions() {
        assert_eq!(frame_width(Some(1.5)), 210.0);
        assert_eq!(frame_width(Some(2.0 / 3.0)), 93.0);
        assert_eq!(frame_width(None), 210.0);
        let layout = StripLayout::new(vec![210.0, 93.0, 210.0]);
        assert_eq!(layout.xs, vec![0.0, 215.0, 313.0]);
        assert_eq!(layout.width, 523.0);
        assert_eq!(layout.middle(1), 261.5);
        assert_eq!(layout.indices(0.0, 100.0), 0..1);
        assert_eq!(layout.indices(212.0, 214.0), 1..1);
        assert_eq!(layout.indices(200.0, 320.0), 0..3);
        assert_eq!(layout.hit(212.0), None);
        assert_eq!(layout.hit(216.0), Some(1));
        assert_eq!(layout.hit(600.0), None);
        assert_eq!(layout.index_at(600.0), Some(2));
        assert_eq!(StripLayout::new(vec![]).index_at(3.0), None);
    }

    #[test]
    fn centring_stops_at_the_rows_ends() {
        assert_eq!(centred(100.0, 800.0, 2000.0), 0.0);
        assert_eq!(centred(1000.0, 800.0, 2000.0), 600.0);
        assert_eq!(centred(1900.0, 800.0, 2000.0), 1200.0);
        assert_eq!(centred(300.0, 800.0, 500.0), 0.0);
        let old = StripLayout::new(vec![210.0; 10]);
        let new = StripLayout::new(vec![93.0; 10]);
        let kept = hold(&old, &new, 5, 700.0, 400.0);
        assert_eq!(kept, 700.0 + new.middle(5) - old.middle(5));
    }

    #[test]
    fn dimming_follows_the_mac() {
        assert_eq!(strip_opacity(true, false, false, false, false), 1.0);
        assert_eq!(strip_opacity(false, true, false, false, false), 1.0);
        assert_eq!(strip_opacity(false, false, false, false, false), STRIP_DIM);
        assert_eq!(strip_opacity(false, false, false, true, false), 0.75);
        assert_eq!(strip_opacity(false, false, false, true, true), 0.9);
        assert_eq!(strip_opacity(false, false, true, true, false), 0.22);
        assert_eq!(strip_opacity(true, false, true, false, false), 0.5);
        assert_eq!(reject_opacity(false, true), 0.4);
    }

    #[test]
    fn sheet_rows_pack_and_fill_the_width() {
        let layout = SheetLayout::new(&[1.5; 7], 1000.0, 176.0);
        // 968 usable: three landscapes at 176 are 792 + 12; a fourth is
        // nearer the chosen height stretched less.
        for row in &layout.rows[..layout.rows.len() - 1] {
            let last = &layout.frames[row.end - 1];
            assert_eq!(last.max_x(), 1000.0 - GRID_SIDE, "full rows end flush");
        }
        let last = layout.rows.last().unwrap();
        assert_eq!(last.height, 176.0, "the last row keeps the chosen height");
        assert_eq!(layout.frames[0].y, GRID_TOP);
        assert_eq!(layout.height, last.y + last.height + GRID_BOTTOM);
        for (i, row) in layout.rows.iter().enumerate().skip(1) {
            assert_eq!(row.y, layout.rows[i - 1].y + layout.rows[i - 1].height + GRID_GAP);
        }
        assert_eq!(SheetLayout::new(&[], 1000.0, 176.0).height, GRID_TOP + GRID_BOTTOM);
    }

    #[test]
    fn sheet_rows_match_the_swift() {
        // SheetLayout(ratios: [1.5, 0.667, 1.5, 1.5, 1.5, 1.5, 0.667, 1.5],
        // width: 1388, target: 176), as Selection.swift computes it.
        let layout = SheetLayout::new(&[1.5, 0.667, 1.5, 1.5, 1.5, 1.5, 0.667, 1.5], 1388.0, 176.0);
        let row = &layout.rows[0];
        // 1356 usable; 1.5+.667+1.5+1.5+1.5 = 6.667 at 176 is 1173 + 24.
        // Adding the next landscape (8.167): (1356-30)/8.167/176 = 0.922,
        // |ln| 0.081; without: (1356-24)/6.667/176 = 1.135, |ln| 0.127.
        assert_eq!((row.start, row.end), (0, 6));
        assert_eq!(row.height, ((1356.0 - 30.0) / 8.167f64).round());
        assert_eq!(layout.rows[1].height, 176.0);
    }

    #[test]
    fn sheet_finds_cells() {
        let layout = SheetLayout::new(&[1.5; 12], 1000.0, 176.0);
        let first = layout.frames[0];
        assert_eq!(layout.hit(first.x + 1.0, first.y + 1.0), Some(0));
        assert_eq!(layout.hit(first.max_x() + 1.0, first.y + 1.0), None);
        assert_eq!(layout.row_of(0), Some(0));
        let second_row = layout.rows[1].start;
        assert_eq!(layout.row_of(second_row), Some(1));
        assert_eq!(layout.row_of(99), None);
        assert_eq!(layout.vertical(0, 1), Some(second_row));
        assert_eq!(layout.vertical(0, -1), None);
        assert_eq!(layout.nearest(-50.0, -50.0), Some(0));
        let band = Rect::spanning((first.x + 5.0, first.y + 5.0), (layout.frames[1].x + 5.0, first.y + 10.0));
        assert_eq!(layout.indices_in(&band), vec![0, 1]);
        let r = layout.indices(layout.rows[1].y, layout.rows[1].y + 1.0);
        assert_eq!(r, layout.rows[1].start..layout.rows[1].end, "the gap above is not the row before");
        assert_eq!(layout.indices(1e6, 2e6), 0..0);
    }

    #[test]
    fn sheet_scrolls_to_reveal_and_centre() {
        let layout = SheetLayout::new(&[1.5; 60], 1000.0, 176.0);
        let viewport = 500.0;
        let i = layout.rows[5].start;
        let scrolled = layout.reveal(i, 0.0, viewport);
        assert_eq!(scrolled, layout.frames[i].max_y() + SELECTION_OUTSET + BOX_MARGIN - viewport);
        assert_eq!(layout.reveal(0, scrolled, viewport), GRID_TOP - SELECTION_OUTSET - BOX_MARGIN);
        assert_eq!(layout.reveal(i, scrolled, viewport), scrolled, "shown already");
        assert_eq!(layout.centre_row(0, viewport), 0.0);
        let last = layout.rows.len() - 1;
        assert_eq!(layout.centre_row(last, viewport), layout.height - viewport);
        let anchored = keep_anchor(&layout, i, 0.5, 100.0, viewport);
        assert_eq!(anchored, layout.frames[i].mid_y() - 100.0);
    }

    #[test]
    fn sizes_clamp_and_slide() {
        assert_eq!(clamp_size(40.0), 100.0);
        assert_eq!(clamp_size(999.0), 360.0);
        assert_eq!(size_at(size_fraction(176.0)), 176.0);
        assert_eq!(size_at(0.0), 100.0);
        assert_eq!(ThumbSize::for_rows(176.0, 1.0), ThumbSize::Medium);
        assert_eq!(ThumbSize::for_rows(176.0, 1.5), ThumbSize::Large);
        assert_eq!(ThumbSize::for_rows(100.0, 2.5), ThumbSize::Medium);
    }

    #[test]
    fn reading_goes_outward_from_the_view() {
        assert_eq!(thumbnail_centre(&[], Some(7)), 7);
        assert_eq!(thumbnail_centre(&[3, 4, 5, 6], None), 5);
        assert_eq!(outward(vec![(1, 'a'), (3, 'b'), (5, 'c'), (2, 'd'), (4, 'e')], 3), vec!['b', 'd', 'e', 'a', 'c']);
        let order = prefetch_order(100, &[40, 41, 42], None);
        assert_eq!(order[..4], [39, 43, 38, 44]);
        assert!(!order.contains(&41));
        assert_eq!(order.len(), 65 - 3);
        assert_eq!(*order.iter().min().unwrap(), 41 - 32);
        assert_eq!(*order.iter().max().unwrap(), 41 + 32);
        assert_eq!(prefetch_order(5, &[], Some(0)), vec![0, 1, 2, 3, 4]);
        assert_eq!(prefetch_order(0, &[], None), Vec::<usize>::new());
    }

    #[test]
    fn the_pump_develops_on_screen_first_then_outward() {
        let item = |on_screen, open, done| PumpItem { look: true, on_screen, open, done };
        let mut items: Vec<PumpItem> = (0..10).map(|_| item(false, false, false)).collect();
        items[2] = item(false, true, false);
        items[6].on_screen = true;
        items[7].on_screen = true;
        items[8] = item(true, false, true);
        items[9].look = false;
        // On screen 6, 7 (8 is done); the rest from the middle of the view (7).
        assert_eq!(pump_order(&items, false), vec![6, 7, 5, 4, 3, 1, 0]);
        assert_eq!(pump_order(&items, true), vec![6, 7]);
        // Nothing on screen: outward from the open photograph.
        for i in &mut items {
            i.on_screen = false;
        }
        assert_eq!(pump_order(&items, false), vec![1, 3, 0, 4, 5, 6, 7]);
        assert_eq!(pump_order(&items, true), Vec::<usize>::new());
    }

    #[test]
    fn pools_follow_the_machine() {
        assert_eq!(helpers(12, 16 << 30, false), 2);
        assert_eq!(helpers(12, 16 << 30, true), 1);
        assert_eq!(helpers(4, 32 << 30, false), 1);
        assert_eq!(helpers(16, 8 << 30, false), 1);
        assert_eq!(readers(12), 4);
        assert_eq!(readers(2), 1);
        assert_eq!(readers(1), 1);
    }

    #[test]
    fn the_cache_lets_the_oldest_go() {
        let mut lru = Lru::new(100, 3);
        lru.insert("a", 1, 40);
        lru.insert("b", 2, 40);
        assert_eq!(lru.get(&"a"), Some(&1));
        lru.insert("c", 3, 40);
        assert!(!lru.contains(&"b"), "over 100 bytes: the least recent goes");
        assert!(lru.contains(&"a") && lru.contains(&"c"));
        assert_eq!(lru.bytes(), 80);
        lru.insert("d", 4, 1);
        lru.insert("e", 5, 1);
        assert_eq!(lru.len(), 3, "the count is bounded too");
        lru.insert("e", 6, 10);
        assert_eq!(lru.peek(&"e"), Some(&6));
        lru.retain(|k, _| *k != "e");
        assert!(!lru.contains(&"e"));
        let total: usize = ["a", "c", "d"].iter().filter(|k| lru.contains(k)).count();
        assert_eq!(lru.len(), total);
        for _ in 0..1000 {
            lru.get(&"d");
        }
        assert!(lru.order.len() <= 2 * lru.len() + 65, "touches don't grow the queue");
        lru.clear();
        assert_eq!((lru.len(), lru.bytes()), (0, 0));
    }

    #[test]
    fn the_sentence_says_what_shows() {
        let all = sentence(&Filter::default(), 12, 12);
        assert_eq!(all, vec![Part::Words("Showing ".into()), Part::Filter { word: "all".into(), on: false }, Part::Words(" 12 photographs".into())]);
        let mut filter = Filter::default();
        filter.rating = RatingFilter::AtLeast(3);
        filter.toggle_flag(FlagFilter::Pick);
        filter.toggle_edits(EditsFilter::Edited);
        let words = sentence(&filter, 4, 12);
        assert_eq!(
            words,
            vec![
                Part::Words("Showing 4 of 12 · ".into()),
                Part::Words("rated ".into()),
                Part::Filter { word: "★★★ or more".into(), on: true },
                Part::Words(", ".into()),
                Part::Filter { word: "picks".into(), on: true },
                Part::Words(", ".into()),
                Part::Filter { word: "edited".into(), on: true },
                Part::Clear,
            ]
        );
        assert_eq!(foot(12, 12, 1, false), "12 photographs · 1 selected");
        assert_eq!(foot(4, 12, 1, true), "4 of 12 photographs");
        assert_eq!(sentence(&Filter::default(), 1, 1)[2], Part::Words(" 1 photograph".into()));
    }

    #[test]
    fn the_flight_eases_and_staggers() {
        assert_eq!(flight_curve(0.0), 0.0);
        assert_eq!(flight_curve(1.0), 1.0);
        // Fast out of the start, as cubic-bezier(.22,.8,.24,1).
        assert!(flight_curve(0.25) > 0.6, "{}", flight_curve(0.25));
        let mut last = 0.0;
        for i in 1..=100 {
            let v = flight_curve(i as f64 / 100.0);
            assert!(v >= last - 1e-9, "monotonic");
            last = v;
        }
        assert!((cubic_bezier(0.25, 0.25, 0.75, 0.75, 0.3) - 0.3).abs() < 1e-6, "linear stays linear");
        assert_eq!(flight_delay(10, Some(10)), 0.0);
        assert_eq!(flight_delay(12, Some(10)), 2.0 * STAGGER_MS);
        assert_eq!(flight_delay(40, Some(10)), 12.0 * STAGGER_MS);
        let (a, b) = (Rect::new(0.0, 500.0, 210.0, 140.0), Rect::new(16.0, 24.0, 264.0, 176.0));
        assert_eq!(flyer_at(&a, &b, 0.0, 0.0), a);
        assert_eq!(flyer_at(&a, &b, FLIGHT_MS, 0.0), b);
        assert_eq!(flyer_at(&a, &b, STAGGER_MS, 2.0 * STAGGER_MS), a, "waiting for its turn");
        let window = Rect::new(0.0, 38.0, 1400.0, 900.0);
        let band = Rect::new(0.0, 752.0, 1000.0, 186.0);
        assert_eq!(mask_at(&band, &window, 0.0), band);
        assert_eq!(mask_at(&band, &window, FLIGHT_MS), window);
        let mid = mask_at(&band, &window, FLIGHT_MS / 2.0);
        assert!(mid.y >= window.y && mid.y < band.y && mid.max_y() <= window.max_y() + 1e-9);
        assert_eq!(flight_length(12.0 * STAGGER_MS), 396.0);
    }

    #[test]
    fn rects_lerp_and_span() {
        let r = Rect::spanning((10.0, 20.0), (0.0, 5.0));
        assert_eq!(r, Rect::new(0.0, 5.0, 10.0, 15.0));
        assert!(r.intersects(&Rect::new(9.0, 19.0, 5.0, 5.0)));
        assert!(!r.intersects(&Rect::new(10.0, 5.0, 5.0, 5.0)), "touching edges don't count");
        assert_eq!(r.lerp(&Rect::new(10.0, 15.0, 20.0, 25.0), 0.5), Rect::new(5.0, 10.0, 15.0, 20.0));
    }
}
