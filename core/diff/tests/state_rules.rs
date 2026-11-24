//! Phase 4a's state rules (core/src/state, through negs_core_rules::state)
//! against the frozen windows/src/state.rs (core/diff/oracle/state.rs, as
//! on b0f1f5b): stocks and colour filters read from the real catalog
//! (state_common/catalog.json, spektrafilm-native's reply) and from random
//! entries, Fotufilm films, the filter link, grain lanes, choosing a stock
//! and pairing a film on random edit states, the menus' groups and the film
//! search, option titles, snapping, values, film formats and ratios. Every
//! output compared, floats bit for bit. Seeded (NEGS_RULES_SEEDS cases,
//! default 10,000; NEGS_RULES_SEED the first).
//!
//! The film search folds text with the oracle's FindNLSStringEx stand-in
//! on both sides: the core asks it through negs_text_set_contains, as the
//! Windows app registers Win32's.

mod rules_common;
mod state_common;

use negs_core_rules::state as c;
use negs_state_oracle::state as o;
use rules_common::{Leaks, Random, Tally, b, cases, first_seed};
use serde_json::{Value, json};
use state_common::{edit_state, engine_defaults, serial, string, value};

fn catalog() -> Value {
    serde_json::from_str(include_str!("state_common/catalog.json")).unwrap()
}

/// The oracle's `contains_folded`, word for word, for the core to ask.
unsafe extern "C" fn contains_folded(text: *const u8, text_len: usize, word: *const u8, word_len: usize) -> u8 {
    use windows::Win32::Globalization::{FIND_FROMSTART, FindNLSStringEx, LINGUISTIC_IGNORECASE, LINGUISTIC_IGNOREDIACRITIC, LOCALE_NAME_INVARIANT};
    let utf8 = |data: *const u8, len: usize| -> &str {
        if data.is_null() || len == 0 { "" } else { std::str::from_utf8(unsafe { std::slice::from_raw_parts(data, len) }).unwrap() }
    };
    let (text, word): (Vec<u16>, Vec<u16>) = (utf8(text, text_len).encode_utf16().collect(), utf8(word, word_len).encode_utf16().collect());
    let flags = FIND_FROMSTART | LINGUISTIC_IGNORECASE.0 | LINGUISTIC_IGNOREDIACRITIC.0;
    (unsafe { FindNLSStringEx(LOCALE_NAME_INVARIANT, flags, &text, &word, None, None, None, windows::Win32::Foundation::LPARAM(0)) } >= 0) as u8
}

/// The oracle's stock as the core wrapper's.
fn cs(s: &o::Stock) -> c::Stock {
    c::Stock {
        id: s.id.clone(),
        name: s.name.clone(),
        paper: s.paper,
        positive: s.positive,
        bw: s.bw,
        target: s.target.clone(),
        times: s.times.clone(),
        grain: s.grain.clone(),
        fotufilm: s.fotufilm,
        manufacturer_data: s.manufacturer_data,
        negs_built: s.negs_built,
        cine_print: s.cine_print,
    }
}

/// A stock's every field, numbers by their bits.
fn stock_key(s: &o::Stock) -> String {
    format!("{s:?} times {:?}", s.times.iter().map(|t| t.to_bits()).collect::<Vec<_>>())
}
fn c_stock_key(s: &c::Stock) -> String {
    stock_key(&o::Stock {
        id: s.id.clone(),
        name: s.name.clone(),
        paper: s.paper,
        positive: s.positive,
        bw: s.bw,
        target: s.target.clone(),
        times: s.times.clone(),
        grain: s.grain.clone(),
        fotufilm: s.fotufilm,
        manufacturer_data: s.manufacturer_data,
        negs_built: s.negs_built,
        cine_print: s.cine_print,
    })
}

const WORDS: &[&str] = &[
    "portra", "PORTRA", "Pórträ", "400", "160", "kodak", "fujifilm", "velvia", "slide", "color", "negative", "black", "&", "white", "motion",
    "picture", "print", "paper", "vision", "eterna", "tri-x", "trix", "ilford", "hp5", "-", "_", "x", "é", "ß", "", "cinestill", "800t", "50D",
];
const SPACES: &[&str] = &[" ", "  ", "\t", "\n", "\u{3000}", "\u{a0}", "\u{2028}", "\u{85}", "\u{200b}", "\u{1680}"];

fn query(r: &mut Random, stocks: &[o::Stock]) -> String {
    let n = r.index(4);
    let mut q = String::new();
    if r.chance(4) {
        q.push_str(SPACES[r.index(SPACES.len())]);
    }
    for i in 0..n {
        if i > 0 {
            q.push_str(SPACES[r.index(SPACES.len())]);
        }
        let word = match r.below(5) {
            0 => string(r),
            1 if !stocks.is_empty() => {
                let s = &stocks[r.index(stocks.len())];
                let source = if r.bool() { &s.name } else { &s.id };
                let parts: Vec<&str> = source.split([' ', '_']).collect();
                parts[r.index(parts.len())].to_string()
            }
            _ => WORDS[r.index(WORDS.len())].to_string(),
        };
        q.push_str(&if r.chance(4) { word.to_uppercase() } else { word });
    }
    q
}

fn grain_path(r: &mut Random) -> String {
    let fields = ["rms_granularity", "density_min", "uniformity", "particle_scale_sublayers", "blur", "", "rms_granularity.0"];
    let lanes = ["0", "1", "2", "3", "+1", "01", "-1", "", "18446744073709551615", "18446744073709551616", "1.5", " 1", "٣", "+"];
    let prefix = ["film_render.grain.", "film_render.grain", "film_render.grain..", "x.film_render.grain.", ""][if r.chance(6) { r.index(5) } else { 0 }];
    format!("{prefix}{}.{}", fields[r.index(fields.len())], lanes[r.index(lanes.len())])
}

const OPTIONS: &[&str] = &[
    "average", "median", "center_weighted", "partial", "matrix", "multi_zone", "highlight_weighted", "as_shot", "auto", "daylight", "tungsten", "D55",
    "D50", "D65", "custom", "black_pro_mist", "glimmerglass", "pro_mist", "cinebloom", "off", "oklch", "oklrab", "cam16ucs", "aces_rgc", "xy",
    "hanatos2025", "arctic2026beta04", "mallett2019", "some_new_option", "__", "_a_", "ǆemal_ﬀ", "ß_straße", "ŉ_x", "i̇_ı", "ᾳ_ῼ",
];

fn link(r: &mut Random) -> (o::FilterLink, c::FilterLink) {
    let i = r.index(3);
    (o::FilterLink::ALL[i], c::FilterLink::ALL[i])
}

#[test]
fn state_rules_match_the_frozen_rust() {
    let _guard = serial();
    let leaks = Leaks::start();
    c::set_contains(Some(contains_folded));
    let d = engine_defaults();
    let cat = catalog();
    let profiles: Vec<Value> = cat["profiles"].as_array().unwrap().clone();
    let filters: Vec<Value> = cat["color_filters"].as_array().unwrap().clone();
    let o_stocks: Vec<o::Stock> = profiles.iter().map(o::Stock::new).collect();
    let o_filters: Vec<o::ColorFilter> = filters.iter().map(o::ColorFilter::new).collect();
    let mut t = Tally::default();
    let n = cases();
    for seed in first_seed()..first_seed() + n {
        let r = &mut Random(seed);

        // Stocks: a real profile, one altered, or anything.
        let profile = match r.below(4) {
            0 => profiles[r.index(profiles.len())].clone(),
            1 => {
                let i = r.index(profiles.len());
                state_common::alter(r, &profiles[i], 4)
            }
            2 => value(r, 3),
            _ => {
                let mut p = profiles[r.index(profiles.len())].clone();
                for key in ["paper", "bw", "positive", "fotufilm", "manufacturer_data", "negs_built", "cine_print", "name", "target_print"] {
                    if r.chance(3) {
                        p[key] = state_common::scalar(r);
                    }
                }
                p
            }
        };
        let os = o::Stock::new(&profile);
        let core = c::Stock::new(&profile);
        t.check("Stock::new", seed, stock_key(&os), c_stock_key(&core));
        let s = cs(&os);
        t.check("is_motion_picture_print", seed, os.is_motion_picture_print(), s.is_motion_picture_print());
        let shown = r.bool();
        t.check("lists_negs_built", seed, o::lists_negs_built(&os, shown), c::lists_negs_built(&s, shown));
        let key = ["all", "datasheets", "off", "", "Off", "x"][r.index(6)];
        let (of, cf) = (o::FotufilmFilms::from_key(key), c::FotufilmFilms::from_key(key));
        t.check("FotufilmFilms", seed, (of.key(), of.title(), of.lists(&os)), (cf.key(), cf.title(), cf.lists(&s)));

        // Colour filters.
        let entry = match r.below(3) {
            0 => filters[r.index(filters.len())].clone(),
            1 => {
                let i = r.index(filters.len());
                state_common::alter(r, &filters[i], 3)
            }
            _ => {
                let colour = ["#4dc53f", "4dc53f", "#+1", "#", "#FFFFFFFFF", "#-1", "#ffffff", "##1", "#0x1"][r.index(9)];
                json!({"key": state_common::key(r), "name": string(r), "glass": string(r), "colour": colour})
            }
        };
        let (ofl, cfl) = (o::ColorFilter::new(&entry), c::ColorFilter::new(&entry));
        t.check("ColorFilter::new", seed, (ofl.key.clone(), ofl.name.clone(), ofl.glass.clone(), ofl.colour), (cfl.key.clone(), cfl.name.clone(), cfl.glass.clone(), cfl.colour));
        let fkey = if r.bool() { ofl.key.clone() } else { ["hoya_x1", "hoya_x0", "hoya_y2", "hoya_ya3", "hoya_r1", "none", ""][r.index(7)].to_string() };
        t.check("ColorFilter::effect", seed, o::ColorFilter::effect(&fkey), c::ColorFilter::effect(&fkey));
        let stops = match r.below(4) {
            0 => None,
            1 => Some(r.edge()),
            _ => Some(r.float(0.0, 4.0)),
        };
        let film = if r.bool() { os.name.clone() } else { string(r) };
        let (metered, decimal) = (r.bool(), [',', '.', '٫', '\u{0}', '😀'][r.index(5)]);
        let chosen = r.chance(5);
        let (of_ref, cf_ref) = if chosen { (None, None) } else { (Some(&ofl), Some(&cfl)) };
        t.check("ColorFilter::hint", seed, o::ColorFilter::hint(of_ref, stops, &film, metered, decimal), c::ColorFilter::hint(cf_ref, stops, &film, metered, decimal));
        t.check("ColorFilter::help", seed, o::ColorFilter::help(of_ref), c::ColorFilter::help(cf_ref));
        let real = &o_filters[r.index(o_filters.len())];
        t.check("ColorFilter::help real", seed, o::ColorFilter::help(Some(real)), c::ColorFilter::help(Some(&c::ColorFilter::new(&filters[o_filters.iter().position(|f| f == real).unwrap()]))));

        // The filter link.
        let lkey = ["together", "opposite", "separate", "", "Together"][r.index(5)];
        let (ol, cl) = (o::FilterLink::from_key(lkey), c::FilterLink::from_key(lkey));
        t.check("FilterLink::from_key", seed, (ol.key(), ol.title()), (cl.key(), cl.title()));
        let path = [o::FilterLink::MAGENTA, o::FilterLink::YELLOW, "enlarger.print_exposure", ""][r.index(4)];
        t.check("FilterLink::links", seed, o::FilterLink::links(path), c::FilterLink::links(path));
        let (ol, cl) = link(r);
        let (v, m, y) = (r.float(-60.0, 60.0), r.float(-55.0, 55.0), r.float(-55.0, 55.0));
        let (oa, ob) = ol.shifts(path, v, m, y);
        let (ca, cb) = cl.shifts(path, v, m, y);
        t.check("FilterLink::shifts", seed, (b(oa), b(ob)), (b(ca), b(cb)));

        // Grain lanes.
        let gp = grain_path(r);
        t.check("grain_lane", seed, o::grain_lane(&gp), c::grain_lane(&gp));
        t.check("color_only", seed, o::color_only(&gp), c::color_only(&gp));
        let bp = ["film_render.grain.rms_granularity.0", "film_render.grain.uniformity.0", "film_render.grain.uniformity.1", ""][r.index(4)];
        t.check("bw_title", seed, o::bw_title(&gp), c::bw_title(&gp));
        t.check("bw_title known", seed, o::bw_title(bp), c::bw_title(bp));

        // Choosing on random states, with the real stocks (sometimes a few).
        let ostate = edit_state(r, &d);
        let mut ostate = ostate;
        if r.bool() {
            ostate.film = o_stocks[r.index(o_stocks.len())].id.clone();
        }
        if r.bool() {
            ostate.paper = o_stocks[r.index(o_stocks.len())].id.clone();
        }
        let stocks: Vec<o::Stock> = if r.chance(4) {
            (0..r.index(6)).map(|_| {
                let mut s = o_stocks[r.index(o_stocks.len())].clone();
                if r.chance(3) {
                    s.id = ostate.film.clone();
                }
                if r.chance(3) {
                    s.target = ostate.paper.clone();
                }
                s
            }).collect()
        } else {
            o_stocks.clone()
        };
        let c_stocks: Vec<c::Stock> = stocks.iter().map(cs).collect();
        let cstate: c::EditState = serde_json::from_str(&serde_json::to_string(&ostate).unwrap()).unwrap();
        let id = if r.chance(5) { string(r) } else { stocks.get(r.index(stocks.len().max(1))).map_or(String::new(), |s| s.id.clone()) };
        let (paper, keep) = (r.bool(), r.bool());
        let chosen_o = o::choosing(&ostate, &id, paper, keep, &stocks);
        let chosen_c = c::choosing(&cstate, &id, paper, keep, &c_stocks);
        t.check("choosing", seed, serde_json::to_string(&chosen_o).unwrap(), serde_json::to_string(&chosen_c).unwrap());
        let (mut po, mut pc) = (ostate.clone(), cstate.clone());
        let with_paper = r.bool();
        o::pair_film(&mut po, &stocks, with_paper);
        c::pair_film(&mut pc, &c_stocks, with_paper);
        t.check("pair_film", seed, serde_json::to_string(&po).unwrap(), serde_json::to_string(&pc).unwrap());

        // Groups and the search.
        let films_o: Vec<&o::Stock> = stocks.iter().filter(|s| !s.paper || r.chance(8)).collect();
        let papers_o: Vec<&o::Stock> = stocks.iter().filter(|s| s.paper || r.chance(8)).collect();
        let pick = |list: &[&o::Stock]| -> Vec<usize> { list.iter().map(|s| stocks.iter().position(|x| std::ptr::eq(x, *s)).unwrap()).collect() };
        let (fi, pi) = (pick(&films_o), pick(&papers_o));
        let films_c: Vec<&c::Stock> = fi.iter().map(|&i| &c_stocks[i]).collect();
        let papers_c: Vec<&c::Stock> = pi.iter().map(|&i| &c_stocks[i]).collect();
        let og = |g: Vec<o::StockGroup>| -> Vec<(&'static str, Vec<String>)> { g.into_iter().map(|g| (g.title, g.stocks.iter().map(|s| s.id.clone()).collect())).collect() };
        let cg = |g: Vec<c::StockGroup>| -> Vec<(&'static str, Vec<String>)> { g.into_iter().map(|g| (g.title, g.stocks.iter().map(|s| s.id.clone()).collect())).collect() };
        t.check("film_groups", seed, og(o::film_groups(&films_o, &papers_o)), cg(c::film_groups(&films_c, &papers_c)));
        t.check("paper_groups", seed, og(o::paper_groups(&papers_o)), cg(c::paper_groups(&papers_c)));
        let q = query(r, &stocks);
        t.check("matching films", seed, og(o::matching(&q, o::film_groups(&films_o, &papers_o))), cg(c::matching(&q, c::film_groups(&films_c, &papers_c))));
        t.check("matching papers", seed, og(o::matching(&q, o::paper_groups(&papers_o))), cg(c::matching(&q, c::paper_groups(&papers_c))));

        // Words and values.
        let option = match r.below(3) {
            0 => OPTIONS[r.index(OPTIONS.len())].to_string(),
            1 => string(r),
            _ => (0..r.index(4)).map(|_| OPTIONS[r.index(OPTIONS.len())]).collect::<Vec<_>>().join("_"),
        };
        t.check("option_title", seed, o::option_title(&option), c::option_title(&option));
        let step = match r.below(4) {
            0 => [1.0, 0.1, 0.01, 0.001, 0.05, 0.5, 0.25, 5.0, 0.0001, 1e-6, 1e-7][r.index(11)],
            1 => r.edge(),
            _ => r.float(0.0001, 10.0),
        };
        // f64::clamp panics on bounds the wrong way round or NaN: the
        // controls' ranges never are.
        let (low, high) = (r.float(-200.0, 100.0), r.float(-100.0, 25000.0));
        let (low, high) = if low.is_nan() || high.is_nan() { (0.0, 1.0) } else { (low.min(high), low.max(high)) };
        let v = r.float(-300.0, 30000.0);
        t.check("snap", seed, b(o::snap(v, low, high, step)), b(c::snap(v, low, high, step)));
        t.check("decimals", seed, o::decimals(step), c::decimals(step));
        let fv = if r.chance(3) { state_common::double(r) } else { v };
        t.check("format_value", seed, o::format_value(fv, step), c::format_value(fv, step));
        let mm = if r.bool() { o::FILM_FORMATS[r.index(o::FILM_FORMATS.len())].1 + r.float(-0.06, 0.06) } else { r.float(0.0, 130.0) };
        t.check("film_format", seed, o::film_format(mm), c::film_format(mm));
        let (w, h) = (r.whole(0.0, 8000.0), r.whole(0.0, 8000.0));
        t.check("ratio_name", seed, o::ratio_name(w, h), c::ratio_name(w, h));
    }

    // Every scalar upper-cases as Rust does (option_title's first letters).
    for scalar in (0..=0x10FFFFu32).filter_map(char::from_u32) {
        let text: String = [scalar, 'x', '_', scalar].iter().collect();
        t.check("option_title every scalar", scalar as u64, o::option_title(&text), c::option_title(&text));
    }

    // The tables.
    t.check("ADJUSTMENT_GROUPS", 0, tables_o(), tables_c());
    t.check("lists", 0, (o::METERING, o::INPUT_MODES, o::ASSUMED_SPACES, o::MANUAL_SPACES, o::WHITE_BALANCE), (c::METERING, c::INPUT_MODES, c::ASSUMED_SPACES, c::MANUAL_SPACES, c::WHITE_BALANCE));
    t.check("paths", 0, (o::GRAIN_STOCK_FIELDS, o::DIR_HALATION_STOCK_PATHS), (c::GRAIN_STOCK_FIELDS, c::DIR_HALATION_STOCK_PATHS));
    t.check("formats", 0, format!("{:?}", o::FILM_FORMATS), format!("{:?}", c::FILM_FORMATS));
    t.check("constants", 0, (o::NEGS_BUILT_HINT, o::PREP_EXPOSURE_RANGE, o::PREP_EXPOSURE_STEP, o::FilterLink::RANGE), (c::NEGS_BUILT_HINT, c::PREP_EXPOSURE_RANGE, c::PREP_EXPOSURE_STEP, c::FilterLink::RANGE));
    c::set_contains(None);
    t.finish("state rules", n);
    leaks.check("state rules");
}

fn tables_o() -> String {
    let groups: Vec<String> = o::ADJUSTMENT_GROUPS.iter().map(|g| format!("{} {:?} {:?} {} {:?}", g.title, g.section, g.enable, g.basic, g.controls)).collect();
    format!("{groups:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?}", o::PREP_TONE, o::PREP_COLOR, o::PREP_DETAIL, o::NEGATIVE_EXPOSURE, o::LENS_BLUR, o::WB_TEMPERATURE, o::WB_TINT, o::FILM_FORMAT)
}

fn tables_c() -> String {
    let groups: Vec<String> = c::ADJUSTMENT_GROUPS.iter().map(|g| format!("{} {:?} {:?} {} {:?}", g.title, g.section, g.enable, g.basic, g.controls)).collect();
    format!("{groups:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?}", c::PREP_TONE, c::PREP_COLOR, c::PREP_DETAIL, c::NEGATIVE_EXPOSURE, c::LENS_BLUR, c::WB_TEMPERATURE, c::WB_TINT, c::FILM_FORMAT)
}
