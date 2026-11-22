//! sheet_rules, the Odin core (negs-core-rules) against the frozen Rust
//! (oracle/sheet_rules.rs, windows/src at 00b0d0d): seeded strips, grids,
//! thumbnail orders, filters, curves and scroll bars, every output compared,
//! floats bit for bit (NaNs as NaNs). NEGS_RULES_SEEDS cases (10,000).

mod rules_common;

use negs_core_diff::sheet_rules as oracle;
use negs_core_rules::sheet_rules as core;
use negs_library::{EditsFilter, Filter, FlagFilter, RatingFilter};
use rules_common::{Leaks, Random, Tally, b, bs, cases, first_seed};

fn rect_o(r: &oracle::Rect) -> [u64; 4] {
    bs([r.x, r.y, r.w, r.h])
}

fn rect_c(r: &core::Rect) -> [u64; 4] {
    bs([r.x, r.y, r.w, r.h])
}

/// A photograph's width ÷ height: the usual shapes, any shape, or awkward.
fn ratio(r: &mut Random) -> f64 {
    match r.below(10) {
        0..=3 => [1.5, 2.0 / 3.0, 1.0, 4.0 / 3.0, 0.75, 16.0 / 9.0, 3.0, 0.2][r.index(8)],
        4..=7 => r.between(0.05, 12.0),
        _ => r.edge(),
    }
}

fn strip_widths(r: &mut Random) -> Vec<f64> {
    let most = if r.chance(10) { 400 } else { 30 };
    let n = if r.chance(20) { 0 } else { r.index(most) };
    (0..n)
        .map(|_| match r.below(8) {
            0 => r.edge(),
            1 => oracle::frame_width(None),
            _ => oracle::frame_width(Some(ratio(r))),
        })
        .collect()
}

fn filter(r: &mut Random) -> Filter {
    let rating = match r.below(4) {
        0 | 1 => RatingFilter::Any,
        2 => RatingFilter::AtLeast(1 + r.below(5) as u8),
        _ => RatingFilter::Unrated,
    };
    let flags = [FlagFilter::Pick, FlagFilter::Unflagged, FlagFilter::Reject].into_iter().filter(|_| r.chance(3)).collect();
    let edits = [EditsFilter::Any, EditsFilter::Any, EditsFilter::Edited, EditsFilter::Unedited][r.index(4)];
    Filter { rating, flags, edits }
}

fn parts_o(p: Vec<oracle::Part>) -> Vec<(u8, String, bool)> {
    p.into_iter()
        .map(|p| match p {
            oracle::Part::Words(w) => (0, w, false),
            oracle::Part::Filter { word, on } => (1, word, on),
            oracle::Part::Clear => (2, String::new(), false),
        })
        .collect()
}

fn parts_c(p: Vec<core::Part>) -> Vec<(u8, String, bool)> {
    p.into_iter()
        .map(|p| match p {
            core::Part::Words(w) => (0, w, false),
            core::Part::Filter { word, on } => (1, word, on),
            core::Part::Clear => (2, String::new(), false),
        })
        .collect()
}

fn thumb_o(t: oracle::ThumbSize) -> u8 {
    t as u8
}

fn thumb_c(t: core::ThumbSize) -> u8 {
    t as u8
}

fn index_list(r: &mut Random, count: usize) -> Vec<usize> {
    let most = if r.chance(5) { 120 } else { 12 };
    let n = r.index(most);
    let mut v: Vec<usize> = (0..n).map(|_| if r.chance(20) { r.next() as usize } else { r.index(count.max(1) + 3) }).collect();
    if r.bool() {
        v.sort();
        v.dedup();
    }
    v
}

fn strip(r: &mut Random, seed: u64, t: &mut Tally) {
    for (in_strip, current) in [(false, false), (false, true), (true, false), (true, true)] {
        t.check("reject_opacity", seed, b(oracle::reject_opacity(in_strip, current)), b(core::reject_opacity(in_strip, current)));
    }
    let bits = r.next();
    let f = |i: u32| bits >> i & 1 == 1;
    t.check("strip_opacity", seed, b(oracle::strip_opacity(f(0), f(1), f(2), f(3), f(4))), b(core::strip_opacity(f(0), f(1), f(2), f(3), f(4))));

    let known = if r.bool() { Some(ratio(r)) } else { None };
    let thumbnail = if r.bool() { known.map_or(ratio(r), |k| k * r.between(0.97, 1.03)) } else { ratio(r) };
    t.check("kept_ratio", seed, b(oracle::kept_ratio(known, thumbnail)), b(core::kept_ratio(known, thumbnail)));
    let ratio_in = if r.chance(5) { None } else { Some(ratio(r)) };
    t.check("frame_width", seed, b(oracle::frame_width(ratio_in)), b(core::frame_width(ratio_in)));

    let widths = strip_widths(r);
    let (o, c) = (oracle::StripLayout::new(widths.clone()), core::StripLayout::new(widths.clone()));
    t.check("StripLayout::new", seed, (o.xs.iter().map(|x| b(*x)).collect::<Vec<_>>(), b(o.width), o.count()), (c.xs.iter().map(|x| b(*x)).collect::<Vec<_>>(), b(c.width), c.count()));
    for _ in 0..4 {
        let (lo, hi) = (r.float(-200.0, o.width.max(1.0) + 200.0), r.float(-200.0, o.width.max(1.0) + 200.0));
        let (min_x, max_x) = if r.chance(4) { (hi, lo) } else { (lo.min(hi), lo.max(hi)) };
        t.check("StripLayout::indices", seed, o.indices(min_x, max_x), c.indices(min_x, max_x));
        let x = if !o.xs.is_empty() && r.bool() {
            let i = r.index(o.xs.len());
            o.xs[i] + [0.0, -0.0, o.widths[i], o.widths[i] - 1e-9, -oracle::STRIP_GAP / 2.0, r.between(-10.0, 10.0)][r.index(6)]
        } else {
            r.float(-300.0, o.width.max(1.0) + 300.0)
        };
        t.check("StripLayout::hit", seed, o.hit(x), c.hit(x));
        t.check("StripLayout::index_at", seed, o.index_at(x), c.index_at(x));
    }
    if !o.xs.is_empty() {
        let i = r.index(o.xs.len());
        t.check("StripLayout::middle", seed, b(o.middle(i)), b(c.middle(i)));
        // The row laid out anew: some shapes read.
        let mut again = widths.clone();
        for w in again.iter_mut() {
            if r.chance(3) {
                *w = oracle::frame_width(Some(ratio(r)));
            }
        }
        let (o2, c2) = (oracle::StripLayout::new(again.clone()), core::StripLayout::new(again));
        let (scroll, visible) = (r.float(-100.0, o.width + 100.0), r.whole(0.0, 3000.0));
        t.check("hold", seed, b(oracle::hold(&o, &o2, i, scroll, visible)), b(core::hold(&c, &c2, i, scroll, visible)));
    }
    let (middle, visible, content) = (r.float(-100.0, 5000.0), r.whole(0.0, 3000.0), r.whole(0.0, 8000.0));
    t.check("centred", seed, b(oracle::centred(middle, visible, content)), b(core::centred(middle, visible, content)));
    let along = r.float(-0.2, 1.2);
    t.check("smoothstep", seed, b(oracle::smoothstep(along)), b(core::smoothstep(along)));
}

fn grid(r: &mut Random, seed: u64, t: &mut Tally) {
    let size = r.float(0.0, 500.0);
    t.check("clamp_size", seed, b(oracle::clamp_size(size)), b(core::clamp_size(size)));
    let most = if r.chance(10) { 600 } else { 40 };
    let n = if r.chance(20) { 0 } else { r.index(most) };
    let ratios: Vec<f64> = (0..n).map(|_| ratio(r)).collect();
    let width = if r.chance(10) { r.edge() } else { r.whole(0.0, 4000.0) };
    let chosen = if r.chance(10) { r.edge() } else { r.whole(80.0, 400.0) };
    let (o, c) = (oracle::SheetLayout::new(&ratios, width, chosen), core::SheetLayout::new(&ratios, width, chosen));
    let frames_o: Vec<[u64; 4]> = o.frames.iter().map(rect_o).collect();
    let frames_c: Vec<[u64; 4]> = c.frames.iter().map(rect_c).collect();
    let rows_o: Vec<(usize, usize, u64, u64)> = o.rows.iter().map(|w| (w.start, w.end, b(w.y), b(w.height))).collect();
    let rows_c: Vec<(usize, usize, u64, u64)> = c.rows.iter().map(|w| (w.start, w.end, b(w.y), b(w.height))).collect();
    t.check("SheetLayout::new", seed, (frames_o, rows_o, b(o.width), b(o.height)), (frames_c, rows_c, b(c.width), b(c.height)));

    let pick = |r: &mut Random| if n > 0 && !r.chance(8) { r.index(n) } else { r.index(n + 3) };
    for _ in 0..4 {
        let i = pick(r);
        t.check("SheetLayout::row_of", seed, o.row_of(i), c.row_of(i));
        let by = [-1isize, 1, -2, 2, 0, -1000, 1000, isize::MIN, isize::MAX][r.index(9)];
        t.check("SheetLayout::vertical", seed, o.vertical(i, by), c.vertical(i, by));
        let (x, y) = if n > 0 && r.bool() {
            let f = &o.frames[r.index(n)];
            (f.x + [0.0, f.w, f.w / 2.0, -3.0, f.w + 3.0][r.index(5)], f.y + [0.0, f.h, f.h / 2.0, -3.0, f.h + 3.0][r.index(5)])
        } else {
            (r.float(-50.0, o.width.max(1.0) + 50.0), r.float(-50.0, o.height + 50.0))
        };
        t.check("SheetLayout::hit", seed, o.hit(x, y), c.hit(x, y));
        t.check("SheetLayout::nearest", seed, o.nearest(x, y), c.nearest(x, y));
        let (a, z) = ((r.float(-50.0, o.width + 50.0), r.float(-50.0, o.height + 50.0)), (x, y));
        let (ro, rc) = (oracle::Rect::spanning(a, z), core::Rect::spanning(a, z));
        t.check("Rect::spanning", seed, rect_o(&ro), rect_c(&rc));
        t.check("SheetLayout::indices_in", seed, o.indices_in(&ro), c.indices_in(&rc));
        let (lo, hi) = (r.float(-100.0, o.height + 100.0), r.float(-100.0, o.height + 100.0));
        t.check("SheetLayout::indices", seed, o.indices(lo, hi), c.indices(lo, hi));
        let viewport = r.whole(0.0, 2000.0);
        let scroll = r.float(-100.0, o.height + 100.0);
        if !o.rows.is_empty() {
            let row = r.index(o.rows.len());
            t.check("SheetLayout::centre_row", seed, b(o.centre_row(row, viewport)), b(c.centre_row(row, viewport)));
        }
        if n > 0 {
            let i = r.index(n);
            t.check("SheetLayout::reveal", seed, b(o.reveal(i, scroll, viewport)), b(c.reveal(i, scroll, viewport)));
            let (down, from_top) = (r.float(0.0, 1.0), r.float(-200.0, 1500.0));
            t.check("keep_anchor", seed, b(oracle::keep_anchor(&o, i, down, from_top, viewport)), b(core::keep_anchor(&c, i, down, from_top, viewport)));
        }
    }
}

fn thumbnails(r: &mut Random, seed: u64, t: &mut Tally) {
    for (so, sc) in oracle::ThumbSize::ALL.into_iter().zip(core::ThumbSize::ALL) {
        t.check("ThumbSize::pixels", seed, so.pixels(), sc.pixels());
        t.check("ThumbSize::cache_limits", seed, so.cache_limits(), sc.cache_limits());
    }
    let (points, scale) = (r.float(0.0, 700.0), [1.0, 1.25, 1.5, 2.0, 3.0][r.index(5)] * if r.chance(10) { r.edge() } else { 1.0 });
    t.check("ThumbSize::grid", seed, thumb_o(oracle::ThumbSize::grid(points, scale)), thumb_c(core::ThumbSize::grid(points, scale)));
    t.check("ThumbSize::for_rows", seed, thumb_o(oracle::ThumbSize::for_rows(points, scale)), thumb_c(core::ThumbSize::for_rows(points, scale)));

    let most = if r.chance(4) { 2000 } else { 80 };
    let count = if r.chance(10) { 0 } else { r.index(most) };
    let on_screen = index_list(r, count);
    let open = if r.bool() { Some(if r.chance(20) { r.next() as usize } else { r.index(count + 2) }) } else { None };
    t.check("thumbnail_centre", seed, oracle::thumbnail_centre(&on_screen, open), core::thumbnail_centre(&on_screen, open));
    t.check("prefetch_order", seed, oracle::prefetch_order(count, &on_screen, open), core::prefetch_order(count, &on_screen, open));

    let items: Vec<(usize, u32)> = (0..r.index(60)).map(|k| (if r.chance(15) { r.next() as usize } else { r.index(40) }, k as u32)).collect();
    let centre = if r.chance(15) { r.next() as usize } else { r.index(45) };
    t.check("outward", seed, oracle::outward(items.clone(), centre), core::outward(items, centre));

    let flags: Vec<u64> = (0..count.min(600)).map(|_| r.next()).collect();
    let open_at = if r.bool() && !flags.is_empty() { Some(r.index(flags.len())) } else { None };
    let pump_o: Vec<oracle::PumpItem> = flags
        .iter()
        .enumerate()
        .map(|(k, f)| oracle::PumpItem { look: f % 5 != 0, on_screen: f >> 8 & 3 == 0, open: Some(k) == open_at || f >> 16 & 63 == 0, done: f >> 24 & 3 == 0 })
        .collect();
    let pump_c: Vec<core::PumpItem> = pump_o.iter().map(|p| core::PumpItem { look: p.look, on_screen: p.on_screen, open: p.open, done: p.done }).collect();
    let low_power = r.chance(4);
    t.check("pump_order", seed, oracle::pump_order(&pump_o, low_power), core::pump_order(&pump_c, low_power));

    let cores = if r.chance(10) { r.next() as usize } else { r.index(40) };
    let memory = if r.chance(10) { r.next() } else { r.below(64) << 30 };
    t.check("helpers", seed, oracle::helpers(cores, memory, low_power), core::helpers(cores, memory, low_power));
    t.check("readers", seed, oracle::readers(cores), core::readers(cores));
}

/// The cache is Rust on both sides (a host container): the same operations
/// give the same answers.
fn lru(r: &mut Random, seed: u64, t: &mut Tally) {
    let (bytes, count) = (r.index(5000), r.index(30));
    let (mut o, mut c) = (oracle::Lru::<u8, u32>::new(bytes, count), core::Lru::<u8, u32>::new(bytes, count));
    let mut seen = Vec::new();
    for _ in 0..r.index(200) {
        let key = r.below(40) as u8;
        match r.below(7) {
            0 | 1 => {
                let (v, cost) = (r.next() as u32, r.index(600));
                o.insert(key, v, cost);
                c.insert(key, v, cost);
            }
            2 => seen.push((o.get(&key).copied(), c.get(&key).copied())),
            3 => seen.push((o.peek(&key).copied(), c.peek(&key).copied())),
            4 => seen.push((o.remove(&key), c.remove(&key))),
            _ => {
                if r.chance(10) {
                    o.clear();
                    c.clear();
                }
            }
        }
        // len, bytes and retain are the windows tests' only (cfg(test) there).
        for k in 0..40 {
            seen.push((Some(o.contains(&k) as u32), Some(c.contains(&k) as u32)));
        }
    }
    let (a, z): (Vec<_>, Vec<_>) = seen.into_iter().unzip();
    t.check("Lru", seed, a, z);
}

fn words(r: &mut Random, seed: u64, t: &mut Tally) {
    let f = filter(r);
    // Counts up to i64::MAX: the library formats them as Odin ints (more
    // photographs than that would not fit in memory).
    let total = if r.chance(10) { (r.next() >> 1) as usize } else { r.index(3000) };
    let shown = if r.bool() { r.index(total.saturating_add(1).max(1)) } else { r.index(3) };
    t.check("sentence", seed, parts_o(oracle::sentence(&f, shown, total)), parts_c(core::sentence(&f, shown, total)));
    let selected = r.index(shown + 3);
    let filtered = r.bool();
    t.check("foot", seed, oracle::foot(shown, total, selected, filtered), core::foot(shown, total, selected, filtered));
    let size = r.float(50.0, 420.0);
    t.check("size_fraction", seed, b(oracle::size_fraction(size)), b(core::size_fraction(size)));
    let fraction = r.float(-0.2, 1.2);
    t.check("size_at", seed, b(oracle::size_at(fraction)), b(core::size_at(fraction)));
}

fn motion(r: &mut Random, seed: u64, t: &mut Tally) {
    let along = r.float(-20.0, 170.0);
    let (lo, lc) = (oracle::sheet_look(along), core::sheet_look(along));
    t.check("sheet_look", seed, bs([lo.0, lo.1]), bs([lc.0, lc.1]));
    let (x1, y1, x2, y2) = (r.float(0.0, 1.0), r.float(-0.5, 1.5), r.float(0.0, 1.0), r.float(-0.5, 1.5));
    let time = r.float(-0.1, 1.1);
    t.check("cubic_bezier", seed, b(oracle::cubic_bezier(x1, y1, x2, y2, time)), b(core::cubic_bezier(x1, y1, x2, y2, time)));
    let length = if r.chance(10) { r.edge() } else { r.whole(0.0, 600.0) };
    let pieces = if r.chance(10) { 0 } else { r.index(40) };
    let kind = r.below(3);
    let f = move |s: f64| match kind {
        0 => oracle::cubic_bezier(x1, y1, x2, y2, s / length.max(1e-9)),
        1 => oracle::sheet_look(s).1,
        _ => x1 * s * s - y1 * s + x2,
    };
    let (po, pc) = (oracle::cubic_pieces(f, length, pieces), core::cubic_pieces(f, length, pieces));
    t.check("cubic_pieces", seed, po.into_iter().map(bs).collect::<Vec<_>>(), pc.into_iter().map(bs).collect::<Vec<_>>());

    let (at, to, dt) = (r.float(-500.0, 5000.0), r.float(-500.0, 5000.0), r.float(0.0, 100.0));
    let to = if r.chance(5) { at + r.between(-0.6, 0.6) } else { to };
    t.check("wheel_glide", seed, b(oracle::wheel_glide(at, to, dt)), b(core::wheel_glide(at, to, dt)));
    let (content, viewport) = (r.whole(0.0, 20000.0), r.whole(0.0, 2000.0));
    let scroll = r.float(-100.0, content + 100.0);
    let k = |v: Option<(f64, f64)>| v.map(|(a, z)| bs([a, z]));
    t.check("bar_knob", seed, k(oracle::bar_knob(scroll, content, viewport)), k(core::bar_knob(scroll, content, viewport)));
    let top = r.float(-50.0, viewport + 50.0);
    t.check("bar_scroll", seed, b(oracle::bar_scroll(top, content, viewport)), b(core::bar_scroll(top, content, viewport)));
    let since = r.float(-100.0, 1600.0);
    t.check("bar_opacity", seed, b(oracle::bar_opacity(since)), b(core::bar_opacity(since)));
}

#[test]
fn sheet_rules_match_the_oracle() {
    let leaks = Leaks::start();
    let (n, first) = (cases(), first_seed());
    let mut t = Tally::default();
    for seed in first..first + n {
        let mut r = Random(seed);
        strip(&mut r, seed, &mut t);
        grid(&mut r, seed, &mut t);
        thumbnails(&mut r, seed, &mut t);
        lru(&mut r, seed, &mut t);
        words(&mut r, seed, &mut t);
        motion(&mut r, seed, &mut t);
    }
    t.finish("sheet_rules", n);
    leaks.check("sheet_rules");
}

/// Rect's own methods stayed Rust: still the oracle's.
#[test]
fn rects_are_the_oracles() {
    let mut r = Random(7);
    let mut t = Tally::default();
    for seed in 0..cases() {
        let (x, y, w, h) = (r.float(-10.0, 10.0), r.float(-10.0, 10.0), r.float(0.0, 10.0), r.float(0.0, 10.0));
        let (o, c) = (oracle::Rect::new(x, y, w, h), core::Rect::new(x, y, w, h));
        t.check("Rect edges", seed, bs([o.max_x(), o.max_y(), o.mid_x()]), bs([c.max_x(), c.max_y(), c.mid_x()]));
        let (px, py) = (r.float(-10.0, 20.0), r.float(-10.0, 20.0));
        t.check("Rect::contains", seed, o.contains(px, py), c.contains(px, py));
        let (q, z) = (r.float(-10.0, 10.0), r.float(-10.0, 10.0));
        let (oo, oc) = (oracle::Rect::new(q, z, px.abs(), py.abs()), core::Rect::new(q, z, px.abs(), py.abs()));
        t.check("Rect::intersects", seed, o.intersects(&oo), c.intersects(&oc));
    }
    t.finish("sheet_rules rects", cases());
}
