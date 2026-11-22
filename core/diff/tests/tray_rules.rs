//! tray_rules, the Odin core (negs-core-rules) against the frozen Rust
//! (oracle/tray_rules.rs, windows/src at 00b0d0d): seeded rows of groups
//! and favourites, searches, steps, scrolls, hits, words, visits, keys and
//! Esc, every output compared, floats bit for bit. NEGS_RULES_SEEDS cases
//! (10,000).

mod rules_common;

use negs_core_diff::tray_rules as oracle;
use negs_core_rules::tray_rules as core;
use rules_common::{Leaks, Random, Tally, b, bs, cases, first_seed};

const IDS: [&str; 14] = [
    "kodak-portra-400",
    "kodak-gold-200",
    "fuji-superia-400",
    "ilford-hp5",
    "kodak-ektar-100",
    "cinestill-800t",
    "negs.your-negative",
    "kodak-portra-160",
    "fuji-c200",
    "",
    "agfa-vista-200",
    "kodak-2383",
    "fuji-crystal-archive",
    "東京-film",
];
const NAMES: [&str; 8] = ["Portra 400", "Gold 200", "Superia X-TRA 400", "HP5 Plus", "Ektar", "800T", "Ålesund 🎞", ""];
const TITLES: [&str; 8] = ["Kodak", "Fujifilm", "Ilford", "Cinema", "Papers", "Scan", "Favourites", "Ünïcode ✓"];
const SPACES: [&str; 12] = ["", " ", "\t", "\n", "\u{a0}", "\u{3000}", "\u{2028}", "\u{85}", "\u{200b}", "\u{feff}", "\u{1680}", "\u{2009}"];
const WORDS: [&str; 6] = ["portra", "x", "Kodak", "東京", "“q”", "a\u{0301}"];

fn query(r: &mut Random) -> String {
    let mut q = String::new();
    for _ in 0..r.index(4) {
        if r.bool() {
            q.push_str(SPACES[r.index(SPACES.len())]);
        } else {
            q.push_str(WORDS[r.index(WORDS.len())]);
        }
    }
    q
}

fn entries(r: &mut Random, most: usize) -> Vec<(String, String)> {
    (0..r.index(most + 1)).map(|_| (IDS[r.index(IDS.len())].to_string(), NAMES[r.index(NAMES.len())].to_string())).collect()
}

/// A row: some groups (some empty), maybe a favourites group from their
/// stocks, as `sections` gives it or as hosts may pass anything.
fn row(r: &mut Random) -> Vec<(String, Vec<(String, String)>, bool)> {
    let most = if r.chance(8) { 30 } else { 6 };
    let groups = r.index(most);
    let mut out: Vec<(String, Vec<(String, String)>, bool)> = Vec::new();
    for _ in 0..groups {
        let most = if r.chance(10) { 60 } else { 8 };
        out.push((TITLES[r.index(TITLES.len())].to_string(), entries(r, most), r.chance(12)));
    }
    if r.bool() {
        let all: Vec<(String, String)> = out.iter().flat_map(|g| g.1.clone()).collect();
        let favs: Vec<(String, String)> = all.into_iter().filter(|_| r.chance(3)).collect();
        out.insert(0, ("Favourites".into(), favs, true));
    }
    out
}

fn sections_o(row: &[(String, Vec<(String, String)>, bool)]) -> Vec<oracle::Section> {
    row.iter()
        .map(|(t, s, f)| oracle::Section { title: t.clone(), stocks: s.iter().map(|(i, n)| oracle::Entry::new(i.as_str(), n.as_str())).collect(), favourites: *f })
        .collect()
}

fn sections_c(row: &[(String, Vec<(String, String)>, bool)]) -> Vec<core::Section> {
    row.iter()
        .map(|(t, s, f)| core::Section { title: t.clone(), stocks: s.iter().map(|(i, n)| core::Entry::new(i.as_str(), n.as_str())).collect(), favourites: *f })
        .collect()
}

fn shape_o(s: &[oracle::Section]) -> Vec<(String, Vec<(String, String)>, bool)> {
    s.iter().map(|s| (s.title.clone(), s.stocks.iter().map(|e| (e.id.clone(), e.name.clone())).collect(), s.favourites)).collect()
}

fn shape_c(s: &[core::Section]) -> Vec<(String, Vec<(String, String)>, bool)> {
    s.iter().map(|s| (s.title.clone(), s.stocks.iter().map(|e| (e.id.clone(), e.name.clone())).collect(), s.favourites)).collect()
}

fn place_o(p: Option<oracle::Place>) -> Option<(String, bool)> {
    p.map(|p| (p.stock, p.favourite))
}

fn place_c(p: Option<core::Place>) -> Option<(String, bool)> {
    p.map(|p| (p.stock, p.favourite))
}

fn hit_o(h: Option<oracle::Hit>) -> Option<(u8, usize, usize)> {
    h.map(|h| match h {
        oracle::Hit::Tile(s, i) => (0, s, i),
        oracle::Hit::Star(s, i) => (1, s, i),
    })
}

fn hit_c(h: Option<core::Hit>) -> Option<(u8, usize, usize)> {
    h.map(|h| match h {
        core::Hit::Tile(s, i) => (0, s, i),
        core::Hit::Star(s, i) => (1, s, i),
    })
}

fn stock(r: &mut Random) -> String {
    if r.chance(10) { "not-a-stock".into() } else { IDS[r.index(IDS.len())].to_string() }
}

fn rows(r: &mut Random, seed: u64, t: &mut Tally) {
    let q = query(r);
    t.check("searching", seed, oracle::searching(&q), core::searching(&q));
    t.check("submit", seed, oracle::submit(&q) as u8, core::submit(&q) as u8);
    t.check("cancel", seed, oracle::cancel(&q) as u8, core::cancel(&q) as u8);

    let matched = row(r);
    let favourites = entries(r, 6);
    let favs_o: Vec<oracle::Entry> = favourites.iter().map(|(i, n)| oracle::Entry::new(i.as_str(), n.as_str())).collect();
    let favs_c: Vec<core::Entry> = favourites.iter().map(|(i, n)| core::Entry::new(i.as_str(), n.as_str())).collect();
    let (so, sc) = (oracle::sections(sections_o(&matched), favs_o, &q), core::sections(sections_c(&matched), favs_c, &q));
    t.check("sections", seed, shape_o(&so), shape_c(&sc));
    let (paper, read_as) = (r.chance(3), r.chance(4));
    let name = NAMES[r.index(NAMES.len())];
    let (so, sc) = (oracle::with_scan(so, paper, read_as, &q, name), core::with_scan(sc, paper, read_as, &q, name));
    t.check("with_scan", seed, shape_o(&so), shape_c(&sc));

    // Rows as `sections` makes them, or anything a host may pass.
    let (so, sc) = if r.bool() {
        (so, sc)
    } else {
        let any = row(r);
        (sections_o(&any), sections_c(&any))
    };
    let (po, pc) = (oracle::places(&so), core::places(&sc));
    t.check("places", seed, po.iter().map(|p| (p.stock.clone(), p.favourite)).collect::<Vec<_>>(), pc.iter().map(|p| (p.stock.clone(), p.favourite)).collect::<Vec<_>>());
    for _ in 0..3 {
        let current = stock(r);
        let in_favourites = r.bool();
        let offset = match r.below(6) {
            0 => -1,
            1 => 1,
            2 => 0,
            3 => r.below(40) as i32 - 20,
            4 => [i32::MIN, i32::MAX, -100_000, 100_000][r.index(4)],
            _ => r.below(5) as i32 - 2,
        };
        t.check("step", seed, place_o(oracle::step(&current, in_favourites, offset, &po)), place_c(core::step(&current, in_favourites, offset, &pc)));
    }

    for (o, c) in so.iter().zip(&sc) {
        t.check("width", seed, b(oracle::width(o)), b(core::width(c)));
    }
    let (starts_o, starts_c) = (oracle::starts(&so), core::starts(&sc));
    t.check("starts", seed, starts_o.iter().map(|x| b(*x)).collect::<Vec<_>>(), starts_c.iter().map(|x| b(*x)).collect::<Vec<_>>());
    let (content_o, content_c) = (oracle::content_width(&so), core::content_width(&sc));
    t.check("content_width", seed, b(content_o), b(content_c));
    let visible = r.whole(0.0, 3000.0);
    for _ in 0..3 {
        let (start, index) = (r.float(-50.0, content_o + 50.0), r.index(70));
        t.check("tile_left", seed, b(oracle::tile_left(start, index)), b(core::tile_left(start, index)));
        let s = stock(r);
        let section = r.index(so.len() + 2);
        t.check("tile_x", seed, oracle::tile_x(&s, section, &so).map(b), core::tile_x(&s, section, &sc).map(b));
        let first = r.bool();
        t.check("section_of", seed, oracle::section_of(&s, &so, first), core::section_of(&s, &sc, first));
        let scrolled = r.float(-100.0, content_o + 100.0);
        t.check("current", seed, oracle::current(scrolled, &starts_o), core::current(scrolled, &starts_c));
        let at = if !starts_o.is_empty() && r.bool() { starts_o[r.index(starts_o.len())] } else { r.float(-100.0, content_o + 100.0) };
        t.check("centring", seed, b(oracle::centring(at, visible, content_o)), b(core::centring(at, visible, content_o)));
        t.check("jump", seed, b(oracle::jump(at, visible, content_o)), b(core::jump(at, visible, content_o)));
        t.check("clamp", seed, b(oracle::clamp(at, visible, content_o)), b(core::clamp(at, visible, content_o)));
        let (in_favourites, centre) = (r.bool(), r.bool());
        let offset = r.float(-100.0, content_o + 100.0);
        t.check("reveal", seed, b(oracle::reveal(offset, &s, in_favourites, centre, &so, visible)), b(core::reveal(offset, &s, in_favourites, centre, &sc, visible)));
        let delta = match r.below(4) {
            0 => [120, -120, 240, -360][r.index(4)],
            1 => r.below(300) as i32 - 150,
            2 => [i32::MIN, i32::MAX, 0, 1][r.index(4)],
            _ => r.next() as i32,
        };
        let sideways = r.bool();
        t.check("wheel", seed, b(oracle::wheel(offset, delta, sideways, visible, content_o)), b(core::wheel(offset, delta, sideways, visible, content_o)));
        let (from, to, elapsed) = (r.float(-100.0, 4000.0), r.float(-100.0, 4000.0), r.float(-20.0, 300.0));
        t.check("glide", seed, b(oracle::glide(from, to, elapsed)), b(core::glide(from, to, elapsed)));
        let (x, y) = (r.float(-20.0, content_o + 20.0), r.float(-10.0, oracle::HEIGHT + 10.0));
        t.check("hit", seed, hit_o(oracle::hit(&so, x, y)), hit_c(core::hit(&sc, x, y)));
        // Near a star: its edges.
        if !starts_o.is_empty() {
            let k = r.index(starts_o.len());
            let left = oracle::tile_left(starts_o[k], r.index(so[k].stocks.len().max(1)));
            let (sx, sy, sw, sh) = oracle::star_rect(left);
            let (x, y) = (sx + [0.0, sw, sw - 1e-9, -1e-9, sw / 2.0][r.index(5)], sy + [0.0, sh, sh - 1e-9, -1e-9, sh / 2.0][r.index(5)]);
            t.check("hit (star)", seed, hit_o(oracle::hit(&so, x, y)), hit_c(core::hit(&sc, x, y)));
            let (a, z) = (oracle::star_rect(left), core::star_rect(left));
            t.check("star_rect", seed, bs([a.0, a.1, a.2, a.3]), bs([z.0, z.1, z.2, z.3]));
        }
    }
}

fn words(r: &mut Random, seed: u64, t: &mut Tally) {
    let (f, g) = (r.bool(), r.bool());
    t.check("star_help", seed, oracle::star_help(f), core::star_help(f));
    t.check("tile_value", seed, oracle::tile_value(f, g), core::tile_value(f, g));
    t.check("done_help", seed, oracle::done_help(f), core::done_help(f));
    t.check("keep_paper_words", seed, oracle::keep_paper_words(f), core::keep_paper_words(f));
    let mut name = NAMES[r.index(NAMES.len())].to_string();
    if r.chance(10) {
        name = name.repeat(1 + r.index(60));
    }
    t.check("star_label", seed, oracle::star_label(&name, f), core::star_label(&name, f));
    let title = TITLES[r.index(TITLES.len())];
    t.check("index_label", seed, oracle::index_label(title), core::index_label(title));
    let before = if r.bool() { Some(name.as_str()) } else { None };
    t.check("hint", seed, oracle::hint(before), core::hint(before));
    let count = if r.chance(10) { r.next() as usize } else { r.index(4) };
    t.check("matches", seed, oracle::matches(count), core::matches(count));
    let q = query(r);
    t.check("no_match", seed, oracle::no_match(&q, f), core::no_match(&q, f));
    let (image, photo) = (
        (if r.chance(10) { 0 } else { r.below(9000) as u32 }, if r.chance(10) { 0 } else { r.below(9000) as u32 }),
        (r.float(0.0, 9000.0), r.float(0.0, 9000.0)),
    );
    let (a, z) = (oracle::stand_in_size(image, photo), core::stand_in_size(image, photo));
    t.check("stand_in_size", seed, bs([a.0, a.1]), bs([z.0, z.1]));
}

fn visits(r: &mut Random, seed: u64, t: &mut Tally) {
    let (paper, before_id) = (r.bool(), stock(r));
    let mut o = oracle::Visit::new(paper, 0u8, before_id.as_str());
    let mut c = core::Visit::new(paper, 0u8, before_id.as_str());
    o.read_as = r.bool();
    c.read_as = o.read_as;
    t.check("Visit::action", seed, o.action(), c.action());
    let (s, current) = (stock(r), stock(r));
    t.check("Visit::tried", seed, o.tried(&current), c.tried(&current));
    t.check("Visit::shows_before", seed, o.shows_before(&s, &current), c.shows_before(&s, &current));
    let (keep, before, now) = (r.bool(), r.below(3) as u8, r.below(3) as u8);
    let ending_o = match oracle::ending(keep, before, &now, o.action()) {
        oracle::Ending::Nothing => (0, 0, ""),
        oracle::Ending::Step(s, a) => (1, s, a),
        oracle::Ending::Restore(s) => (2, s, ""),
    };
    let ending_c = match core::ending(keep, before, &now, c.action()) {
        core::Ending::Nothing => (0, 0, ""),
        core::Ending::Step(s, a) => (1, s, a),
        core::Ending::Restore(s) => (2, s, ""),
    };
    t.check("ending", seed, ending_o, ending_c);

    let keys_o = [oracle::Key::Escape, oracle::Key::Return, oracle::Key::Left, oracle::Key::Right, oracle::Key::Other];
    let keys_c = [core::Key::Escape, core::Key::Return, core::Key::Left, core::Key::Right, core::Key::Other];
    let k = r.index(5);
    let (command, typing) = (r.chance(3), r.chance(3));
    let tk = |k: oracle::TrayKey| match k {
        oracle::TrayKey::GoBack => (0, 0),
        oracle::TrayKey::Keep => (1, 0),
        oracle::TrayKey::Step(n) => (2, n),
        oracle::TrayKey::Pass => (3, 0),
    };
    let tc = |k: core::TrayKey| match k {
        core::TrayKey::GoBack => (0, 0),
        core::TrayKey::Keep => (1, 0),
        core::TrayKey::Step(n) => (2, n),
        core::TrayKey::Pass => (3, 0),
    };
    t.check("tray_key", seed, tk(oracle::tray_key(keys_o[k], command, typing)), tc(core::tray_key(keys_c[k], command, typing)));

    let bits = r.next();
    let f = |i: u32| bits >> i & 1 == 1;
    let eo = oracle::EscapeContext { alert: f(0), typing: f(1), tray: f(2), cropping: f(3), grid: f(4), image_only: f(5), photograph: f(6) };
    let ec = core::EscapeContext { alert: f(0), typing: f(1), tray: f(2), cropping: f(3), grid: f(4), image_only: f(5), photograph: f(6) };
    t.check("escape", seed, oracle::escape(eo) as u8, core::escape(ec) as u8);
}

#[test]
fn tray_rules_match_the_oracle() {
    let leaks = Leaks::start();
    let (n, first) = (cases(), first_seed());
    let mut t = Tally::default();
    for seed in first..first + n {
        let mut r = Random(seed);
        rows(&mut r, seed, &mut t);
        words(&mut r, seed, &mut t);
        visits(&mut r, seed, &mut t);
    }
    t.finish("tray_rules", n);
    leaks.check("tray_rules");
}

#[test]
fn every_escape_is_the_oracles() {
    let mut t = Tally::default();
    for bits in 0u32..128 {
        let f = |i: u32| bits >> i & 1 == 1;
        let eo = oracle::EscapeContext { alert: f(0), typing: f(1), tray: f(2), cropping: f(3), grid: f(4), image_only: f(5), photograph: f(6) };
        let ec = core::EscapeContext { alert: f(0), typing: f(1), tray: f(2), cropping: f(3), grid: f(4), image_only: f(5), photograph: f(6) };
        t.check("escape", bits as u64, oracle::escape(eo) as u8, core::escape(ec) as u8);
    }
    t.finish("tray_rules escapes", 128);
}
