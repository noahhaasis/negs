//! Phase 4a's edit lists and facts readers (core/src/state/edits and
//! core/src/state/facts) against the frozen windows/src/edits.rs and
//! inspector_view/facts.rs (core/diff/oracle, as on b0f1f5b), on random
//! inspector facts: edit states from the engine's defaults with the
//! controls' paths set on and off their defaults, the real catalog's
//! stocks and colour filters (now and then a few altered ones, or none),
//! preferences, brightness matches and their failures.
//!
//! Both ways the core is asked:
//! - host mode, as Windows asks it: negs_core_rules::edits over the
//!   oracle's InspectorFacts (the Facts impl windows/src/edits.rs has);
//! - native mode, as 4d will: a catalog handle, the state parsed into a
//!   negs_state_t and a negs_facts_input_t, one facts handle updated per
//!   case (sometimes twice, so stale caches show), every reader and edits
//!   call compared.
//!
//! Every output compared, floats bit for bit. Seeded (NEGS_RULES_SEEDS
//! cases, default 10,000; NEGS_RULES_SEED the first).

mod rules_common;
mod state_common;

use negs_core_rules::edits as ce;
use negs_core_rules::state as cs;
use negs_core_sys::state as sys;
use negs_core_sys::{negs_buf_t, take_buf};
use negs_state_oracle::crop::Size;
use negs_state_oracle::edits as oe;
use negs_state_oracle::inspector_view::facts::{Brightness, ColorFilters, InspectorFacts, NegativeFacts, Params, Prefs};
use negs_state_oracle::inspector_view::output::COLOR_SPACES;
use negs_state_oracle::json;
use negs_state_oracle::state::{self as os, Adjustment, ColorFilter, EditState, Section, Stock};
use rules_common::{Leaks, Random, Tally, b, cases, first_seed};
use serde_json::{Value, json};
use state_common::{Defaults, edit_state, engine_defaults, serial, string};

const SECTIONS: [Section; 4] = [Section::Camera, Section::Film, Section::Print, Section::Scan];
const C_SECTIONS: [cs::Section; 4] = [cs::Section::Camera, cs::Section::Film, cs::Section::Print, cs::Section::Scan];

fn catalog() -> Value {
    serde_json::from_str(include_str!("state_common/catalog.json")).unwrap()
}

// MARK: Random facts

/// What one case's InspectorFacts borrow.
struct World {
    state: EditState,
    stocks: Vec<Stock>,
    filters: Vec<ColorFilter>,
    catalog: Value,
    more: String,
    color_space: String,
    output_format: String,
    link: String,
    channel: String,
    failure: Option<(u64, Option<Value>, String)>,
    size_error: Option<String>,
    generation: u64,
    matching: bool,
    is_raw: bool,
    lens_supported: bool,
    has_source: bool,
    photo_size: Size,
    source_size: (u32, u32),
    preview_scale: f64,
    flags: [bool; 7],
    match_new_photos: bool,
    lab_all_controls: bool,
    quality: [u32; 3],
    negative: Option<NegativeFacts>,
}

fn facts<'a>(w: &'a World, defaults: &'a Value) -> InspectorFacts<'a> {
    let params = Params { state: &w.state, defaults, stocks: &w.stocks };
    InspectorFacts {
        params,
        color_filters: ColorFilters { params, filters: &w.filters, catalog: &w.catalog },
        brightness: Brightness { params, is_raw: w.is_raw, failure: w.failure.as_ref(), generation: w.generation, matching: w.matching },
        prefs: Prefs {
            lab_all_controls: w.lab_all_controls,
            more_adjustments: &w.more,
            match_new_photos: w.match_new_photos,
            color_space: &w.color_space,
            output_format: &w.output_format,
            jpeg_quality: w.quality[0],
            png_bit_depth: w.quality[1],
            tiff_bit_depth: w.quality[2],
            filter_shift_link: &w.link,
            tone_graph_channel: &w.channel,
        },
        is_raw: w.is_raw,
        lens_supported: w.lens_supported,
        has_source: w.has_source,
        photo_size: w.photo_size,
        source_size: w.source_size,
        preview_scale: w.preview_scale,
        image_shown: w.flags[0],
        size_error: w.size_error.as_deref(),
        custom_film_format: w.flags[1],
        balancing_print: w.flags[2],
        can_balance_print: w.flags[3],
        can_export: w.flags[4],
        cropping: w.flags[5],
        negative: w.negative.clone(),
    }
}

/// Every path the edit lists read, with the control that edits it.
fn paths() -> Vec<(&'static str, Option<&'static Adjustment>)> {
    let mut out: Vec<(&'static str, Option<&'static Adjustment>)> = vec![];
    for g in os::ADJUSTMENT_GROUPS {
        if let Some(enable) = g.enable {
            out.push((enable, None));
        }
        for c in g.controls {
            if !c.path().is_empty() {
                out.push((c.path(), Some(c)));
            }
        }
    }
    for c in os::PREP_TONE.iter().chain(os::PREP_COLOR).chain(os::PREP_DETAIL) {
        out.push((c.path(), Some(c)));
    }
    for p in oe::WHITE_BALANCE_PATHS.iter().chain(oe::INPUT_PATHS.iter()) {
        out.push((p, None));
    }
    for p in [
        oe::FILM_DEVELOPMENT, oe::PRINT_DEVELOPMENT, os::SIZE_TARGET, os::LENS_CORRECTION, os::PREP_EXPOSURE, "camera.auto_exposure",
        "camera.auto_exposure_method", "io.scan_film", "io.output_cctf_encoding", "camera.exposure_compensation_ev", "camera.film_format_mm",
        "camera.lens_blur_um", ColorFilter::PATH,
    ] {
        out.push((p, None));
    }
    out
}

const CHOICES: &[&str] = &["auto", "as_shot", "average", "matrix", "daylight", "none", "hoya_x1", "sRGB", "off", "custom", ""];

fn path_value(r: &mut Random, d: &Defaults, path: &str, control: Option<&Adjustment>, w: &World) -> Value {
    let default = json::get(&d.defaults, path).clone();
    let near = |r: &mut Random, x: f64| -> f64 {
        // Inside, at, or just past is_modified's tolerance.
        let tol = 1e-6 * x.abs().max(1.0);
        x + tol * [0.0, 0.5, -0.5, 0.999, 1.001, 2.0, -2.0][r.index(7)]
    };
    match r.below(10) {
        0 => Value::Null,
        1 => default,
        2 => match default.as_f64() {
            Some(x) => json!(near(r, x)),
            None => json!(near(r, 0.0)),
        },
        3 => match control {
            Some(Adjustment::Slider { low, high, step, .. }) => {
                let x = r.between(*low, *high);
                json!(if r.bool() { (x / step).round() * step } else { x })
            }
            _ => json!(r.float(-50.0, 50.0)),
        },
        4 => Value::Bool(r.bool()),
        5 => match control {
            Some(Adjustment::Choice { options, .. }) if !options.is_empty() => json!(options[r.index(options.len())]),
            _ if path == ColorFilter::PATH && !w.filters.is_empty() => json!(w.filters[r.index(w.filters.len())].key),
            _ => json!(CHOICES[r.index(CHOICES.len())]),
        },
        6 => json!(r.below(5000)),
        7 => state_common::scalar(r),
        8 => json!(r.edge()).as_f64().map_or(Value::Null, |x| json!(x)),
        _ => state_common::value(r, 2),
    }
}

fn stock_variant(r: &mut Random, real: &[Stock]) -> Vec<Stock> {
    (0..r.index(8))
        .map(|_| {
            let mut s = real[r.index(real.len())].clone();
            if r.chance(3) {
                s.paper = r.bool();
            }
            if r.chance(3) {
                s.positive = r.bool();
            }
            if r.chance(3) {
                s.bw = r.bool();
            }
            if r.chance(4) {
                s.times = (0..r.index(4)).map(|_| r.float(0.0, 30.0)).collect();
            }
            if r.chance(4) {
                s.grain = state_common::value(r, 3);
            }
            s
        })
        .collect()
}

fn world(r: &mut Random, d: &Defaults, kind: usize, real: &(Vec<Stock>, Vec<ColorFilter>, Value), variant: &(Vec<Stock>, Vec<ColorFilter>, Value)) -> World {
    let (stocks, filters, catalog) = match kind {
        0 => real.clone(),
        1 => (vec![], vec![], Value::Null),
        _ => variant.clone(),
    };
    let mut state = edit_state(r, d);
    if r.chance(4) {
        state.params = d.defaults.clone();
    }
    if !stocks.is_empty() && !r.chance(3) {
        let films: Vec<&Stock> = stocks.iter().filter(|s| !s.paper).collect();
        if let Some(s) = films.get(r.index(films.len().max(1))) {
            state.film = s.id.clone();
        }
    } else if !stocks.is_empty() && r.bool() {
        state.film = stocks[r.index(stocks.len())].id.clone();
    }
    let mut w = World {
        state,
        stocks,
        filters,
        catalog,
        more: String::new(),
        color_space: if r.chance(4) { string(r) } else { COLOR_SPACES[r.index(4)].to_string() },
        output_format: string(r),
        link: ["together", "opposite", "separate", "", "x"][r.index(5)].to_string(),
        channel: string(r),
        failure: None,
        size_error: r.chance(4).then(|| string(r)),
        generation: r.below(3),
        matching: r.bool(),
        is_raw: r.bool(),
        lens_supported: r.bool(),
        has_source: r.bool(),
        photo_size: Size::ZERO,
        source_size: (r.below(9000) as u32, r.below(9000) as u32),
        preview_scale: r.float(0.0, 1.0),
        flags: [r.bool(), r.bool(), r.bool(), r.bool(), r.bool(), r.bool(), r.bool()],
        match_new_photos: r.bool(),
        lab_all_controls: r.chance(4),
        quality: [r.below(101) as u32, [8, 16][r.index(2)], [8, 16, 32][r.index(3)]],
        negative: None,
    };
    if !w.state.params.is_object() {
        w.state.params = d.defaults.clone();
    }
    let all = paths();
    let n = match r.below(4) {
        0 => 0,
        1 => r.index(4),
        2 => r.index(16),
        _ => r.index(60),
    };
    for _ in 0..n {
        let (path, control) = all[r.index(all.len())];
        let v = path_value(r, d, path, control, &w);
        json::set(&mut w.state.params, path, v);
    }
    if r.chance(3) {
        json::set(&mut w.state.params, "io.input_interpretation.mode", json!(if r.chance(4) { "manual" } else { "auto" }));
    }
    if r.chance(6) {
        let route = if r.chance(5) { string(r) } else { negs_state_oracle::negative::ROUTE.to_string() };
        json::set(&mut w.state.params, negs_state_oracle::negative::ROUTE_PATH, json!(route));
    }
    // B&W films (one grain lane, other titles, filter stops) and their filters.
    let bw: Vec<&Stock> = w.stocks.iter().filter(|s| s.bw && !s.paper).collect();
    if !bw.is_empty() && r.chance(4) {
        w.state.film = bw[r.index(bw.len())].id.clone();
        for lane in ["rms_granularity", "uniformity", "particle_scale_sublayers"] {
            if r.bool() {
                let path = format!("film_render.grain.{lane}.{}", r.index(3));
                json::set(&mut w.state.params, &path, if r.bool() { json!(r.float(0.0, 30.0)) } else { Value::Null });
            }
        }
    }
    if !w.filters.is_empty() && r.chance(3) {
        let key = w.filters[r.index(w.filters.len())].key.clone();
        json::set(&mut w.state.params, ColorFilter::PATH, json!(key));
    }
    // A match solved for these settings, now and then.
    if w.state.brightness_match.is_some() && r.bool() {
        let inputs = os::match_inputs(&w.state);
        w.state.brightness_match.as_mut().unwrap().inputs = (!inputs.is_null()).then_some(inputs);
    }
    if r.chance(3) {
        let inputs = match r.below(3) {
            0 => None,
            1 => Some(os::match_inputs(&w.state)),
            _ => Some(state_common::value(r, 2)),
        };
        w.failure = Some((r.below(3), inputs, string(r)));
    }
    let mut more: Vec<String> = vec![];
    for _ in 0..r.index(5) {
        more.push(match r.below(4) {
            0 => os::ADJUSTMENT_GROUPS[r.index(os::ADJUSTMENT_GROUPS.len())].title.to_string(),
            1 => oe::section_key(SECTIONS[r.index(4)]),
            2 => string(r),
            _ => String::new(),
        });
    }
    w.more = more.join(",");
    w.photo_size = match r.below(4) {
        0 => Size::ZERO,
        1 => Size::new(r.edge(), r.edge()),
        _ => Size::new(r.whole(0.0, 9000.0), r.whole(0.0, 9000.0)),
    };
    if r.bool() {
        w.negative = Some(NegativeFacts { sentence: string(r), busy: r.bool(), backlight_shown: r.bool(), ..Default::default() });
    }
    w
}

// MARK: Host mode

/// windows/src/edits.rs's Facts impl, over the oracle's InspectorFacts.
struct Host<'a, 'b>(&'b InspectorFacts<'a>);

impl ce::Facts for Host<'_, '_> {
    fn value(&self, path: &str) -> &Value {
        self.0.value(path)
    }
    fn is_modified(&self, path: &str) -> bool {
        self.0.is_modified(path)
    }
    fn film_is_bw(&self) -> bool {
        self.0.film_is_bw()
    }
    fn film_positive(&self) -> bool {
        self.0.stocks().iter().find(|s| !s.paper && s.id == self.0.state().film).is_some_and(|s| s.positive)
    }
    fn border_enabled(&self) -> bool {
        self.0.state().border.enabled
    }
    fn matches_brightness(&self) -> bool {
        self.0.matches_brightness()
    }
    fn exposure_offset(&self) -> f64 {
        self.0.exposure_offset()
    }
    fn exposure_offset_modified(&self) -> bool {
        self.0.exposure_offset_modified()
    }
    fn chosen_color_filter_name(&self) -> Option<&str> {
        self.0.chosen_color_filter().map(|f| f.name.as_str())
    }
    fn prefs(&self) -> ce::Prefs<'_> {
        let p = &self.0.prefs;
        ce::Prefs { lab_all_controls: p.lab_all_controls, more_adjustments: p.more_adjustments, match_new_photos: p.match_new_photos, color_space: p.color_space }
    }
    fn is_raw(&self) -> bool {
        self.0.is_raw
    }
    fn lens_supported(&self) -> bool {
        self.0.lens_supported
    }
}

fn host_mode(t: &mut Tally, seed: u64, f: &InspectorFacts, decimal: char) {
    let h = Host(f);
    for (i, tab) in oe::Tab::ALL.into_iter().enumerate() {
        let ctab = ce::Tab::ALL[i];
        t.check("host count", seed, oe::count(tab, f), ce::count(ctab, &h));
        t.check("host edits", seed, format!("{:?}", oe::edits(tab, f, decimal)), format!("{:?}", ce::edits(ctab, &h, decimal)));
        t.check("host tab title", seed, tab.title(), ctab.title());
    }
    for i in 0..os::ADJUSTMENT_GROUPS.len() {
        t.check("host group_count", seed, oe::group_count(i, f), ce::group_count(i, &h));
        t.check("host group_shows", seed, oe::group_shows(f, i), ce::group_shows(&h, i));
        for place in 0..os::ADJUSTMENT_GROUPS[i].controls.len() {
            t.check("host control_shows", seed, oe::control_shows(f, i, place), ce::control_shows(&h, i, place));
        }
        t.check("host module_more", seed, format!("{:?}", oe::module_more(f, i)), format!("{:?}", ce::module_more(&h, i)));
    }
    for (s, c) in SECTIONS.into_iter().zip(C_SECTIONS) {
        t.check("host section_count", seed, oe::section_count(s, f), ce::section_count(c, &h));
        t.check("host shows_section", seed, oe::shows_section(f, s), ce::shows_section(&h, c));
        t.check("host section_more", seed, format!("{:?}", oe::section_more(f, s)), format!("{:?}", ce::section_more(&h, c)));
    }
    t.check("host others", seed, (oe::scan_film_modified(f), oe::output_count(f), oe::brightness_count(f)), (ce::scan_film_modified(&h), ce::output_count(&h), ce::brightness_count(&h)));
}

// MARK: Native mode

fn st(text: &str) -> sys::negs_str_t {
    sys::negs_str_t { data: text.as_ptr(), len: text.len() }
}

unsafe fn text(s: sys::negs_str_t) -> String {
    if s.len == 0 { String::new() } else { String::from_utf8(unsafe { std::slice::from_raw_parts(s.data, s.len) }.to_vec()).unwrap() }
}

/// A catalog handle on `stocks`, `filters` and `catalog` (kept alive by
/// the grains it borrows while building).
fn catalog_handle(defaults: &Value, stocks: &[Stock], filters: &[ColorFilter], catalog: &Value) -> *mut sys::negs_catalog_t {
    let defaults = serde_json::to_vec(defaults).unwrap();
    let rules: Vec<cs::Stock> = stocks
        .iter()
        .map(|s| cs::Stock {
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
        .collect();
    let grains: Vec<Vec<u8>> = rules.iter().map(|s| serde_json::to_vec(&s.grain).unwrap()).collect();
    let c_stocks: Vec<sys::negs_stock_t> = rules.iter().zip(&grains).map(|(s, g)| cs::sys_stock_with_grain(s, g)).collect();
    let c_filters: Vec<sys::negs_color_filter_t> =
        filters.iter().map(|f| sys::negs_color_filter_t { key: st(&f.key), name: st(&f.name), glass: st(&f.glass), colour: f.colour }).collect();
    let catalog = if catalog.is_null() { vec![] } else { serde_json::to_vec(catalog).unwrap() };
    let h = unsafe {
        sys::negs_catalog_new(defaults.as_ptr(), defaults.len(), c_stocks.as_ptr(), c_stocks.len(), c_filters.as_ptr(), c_filters.len(), catalog.as_ptr(), catalog.len())
    };
    assert!(!h.is_null());
    h
}

/// The input's strings, kept for the update (and, without BORROW,
/// overwritten after it).
struct Strings(Vec<String>);

fn input(w: &World, keep: &mut Strings) -> sys::negs_facts_input_t {
    keep.0 = vec![
        w.more.clone(),
        w.color_space.clone(),
        w.output_format.clone(),
        w.link.clone(),
        w.channel.clone(),
        w.failure.as_ref().and_then(|f| f.1.as_ref()).map_or(String::new(), |v| serde_json::to_string(v).unwrap()),
        w.failure.as_ref().map_or(String::new(), |f| f.2.clone()),
        w.size_error.clone().unwrap_or_default(),
        w.negative.as_ref().map_or(String::new(), |n| n.sentence.clone()),
    ];
    let k = &keep.0;
    let mut i: sys::negs_facts_input_t = unsafe { std::mem::zeroed() };
    i.prefs = sys::negs_facts_prefs_t {
        more_adjustments: st(&k[0]),
        color_space: st(&k[1]),
        output_format: st(&k[2]),
        filter_shift_link: st(&k[3]),
        tone_graph_channel: st(&k[4]),
        jpeg_quality: w.quality[0],
        png_bit_depth: w.quality[1],
        tiff_bit_depth: w.quality[2],
        lab_all_controls: w.lab_all_controls as u8,
        match_new_photos: w.match_new_photos as u8,
    };
    i.photo_size = [w.photo_size.width, w.photo_size.height];
    i.preview_scale = w.preview_scale;
    i.generation = w.generation;
    if let Some((generation, inputs, _)) = &w.failure {
        i.has_failure = 1;
        i.failure_generation = *generation;
        i.failure_has_inputs = inputs.is_some() as u8;
        i.failure_inputs = st(&k[5]);
        i.failure_message = st(&k[6]);
    }
    i.size_error = st(&k[7]);
    i.has_size_error = w.size_error.is_some() as u8;
    if let Some(n) = &w.negative {
        i.has_negative = 1;
        i.negative.sentence = st(&k[8]);
        i.negative.busy = n.busy as u8;
        i.negative.backlight_shown = n.backlight_shown as u8;
    }
    i.source_width = w.source_size.0;
    i.source_height = w.source_size.1;
    i.is_raw = w.is_raw as u8;
    i.lens_supported = w.lens_supported as u8;
    i.has_source = w.has_source as u8;
    i.image_shown = w.flags[0] as u8;
    i.custom_film_format = w.flags[1] as u8;
    i.balancing_print = w.flags[2] as u8;
    i.can_balance_print = w.flags[3] as u8;
    i.can_export = w.flags[4] as u8;
    i.cropping = w.flags[5] as u8;
    i.matching = w.matching as u8;
    i
}

fn json_of(buf: negs_buf_t, ok: u8) -> Option<String> {
    let bytes = take_buf(buf);
    (ok == 1).then(|| serde_json::to_string(&serde_json::from_slice::<Value>(&bytes).unwrap()).unwrap())
}

fn native_mode(t: &mut Tally, seed: u64, r: &mut Random, h: *mut sys::negs_facts_t, f: &InspectorFacts, w: &World, decimal: char) {
    let flag_bits = [
        (f.film_is_bw(), sys::NEGS_FACTS_FILM_BW),
        (Host(f).0.stocks().iter().find(|s| !s.paper && s.id == f.state().film).is_some_and(|s| s.positive), sys::NEGS_FACTS_FILM_POSITIVE),
        (f.state().border.enabled, sys::NEGS_FACTS_BORDER),
        (f.matches_brightness(), sys::NEGS_FACTS_MATCHES),
        (f.brightness.active(), sys::NEGS_FACTS_ACTIVE),
        (f.brightness.pending(), sys::NEGS_FACTS_PENDING),
        (f.brightness.failed_here(), sys::NEGS_FACTS_FAILED_HERE),
        (f.exposure_offset_modified(), sys::NEGS_FACTS_OFFSET_MODIFIED),
        (f.shows_meter(), sys::NEGS_FACTS_SHOWS_METER),
        (f.film_format_custom(), sys::NEGS_FACTS_FILM_FORMAT_CUSTOM),
        (f.is_negative(), sys::NEGS_FACTS_NEGATIVE),
        (f.shows_color_filter(), sys::NEGS_FACTS_COLOR_FILTER_SHOWN),
        (f.has_catalog(), sys::NEGS_FACTS_HAS_CATALOG),
        (f.negative_busy(), sys::NEGS_FACTS_NEGATIVE_BUSY),
    ];
    let expected = flag_bits.iter().filter(|(on, _)| *on).fold(0, |bits, (_, bit)| bits | bit);
    t.check("flags", seed, expected, unsafe { sys::negs_facts_flags(h) });

    let mut br: sys::negs_facts_brightness_t = unsafe { std::mem::zeroed() };
    unsafe { sys::negs_facts_brightness(h, &mut br) };
    let range = f.exposure_offset_range();
    t.check("brightness", seed, [b(f.brightness.matched_ev()), b(f.exposure_offset()), b(range.0), b(range.1)], [b(br.matched_ev), b(br.exposure_offset), b(br.range_low), b(br.range_high)]);
    let mut note = sys::negs_str_t { data: std::ptr::null(), len: 0 };
    let has = unsafe { sys::negs_facts_note(h, &mut note) };
    t.check("note", seed, f.brightness.note(), (has == 1).then(|| unsafe { text(note) }));

    let mut key = sys::negs_str_t { data: std::ptr::null(), len: 0 };
    let index = unsafe { sys::negs_facts_color_filter(h, &mut key) };
    let chosen = f.chosen_color_filter().map_or(-1, |c| w.filters.iter().position(|x| std::ptr::eq(x, c)).unwrap() as i64);
    t.check("color_filter", seed, (f.color_filter().to_string(), chosen), (unsafe { text(key) }, index));
    let stop_keys = [f.color_filter().to_string(), "hoya_x1".into(), "hoya_r1".into(), string(r)];
    for k in &stop_keys {
        let mut out = 0.0;
        let ok = unsafe { sys::negs_facts_color_filter_stops(h, k.as_ptr(), k.len(), &mut out) };
        t.check("color_filter_stops", seed, f.color_filters.stops(k).map(b), (ok == 1).then(|| b(out)));
    }
    let film = f.params.film_stock().map_or(-1, |s| w.stocks.iter().position(|x| std::ptr::eq(x, s)).unwrap() as i64);
    t.check("film_stock", seed, film, unsafe { sys::negs_facts_film_stock(h) });
    let mut keys: Vec<String> = w.more.split(',').map(str::to_string).collect();
    keys.extend([String::new(), "Grain".into(), oe::section_key(Section::Print), string(r)]);
    for k in &keys {
        t.check("revealed", seed, f.prefs.revealed(k), unsafe { sys::negs_facts_revealed(h, k.as_ptr(), k.len()) } == 1);
    }
    t.check("preview_space", seed, f.preview_space(), rules_common_static(unsafe { sys::negs_facts_preview_space(h) }));
    let link = os::FilterLink::ALL.iter().position(|l| *l == f.filter_link()).unwrap() as u32;
    t.check("choices", seed, (f.size_mode() as u32, link), unsafe { (sys::negs_facts_choice(h, 0), sys::negs_facts_choice(h, 1)) });
    for paper in [false, true] {
        let mut n = 0;
        let p = unsafe { sys::negs_facts_stock_times(h, paper as u8, &mut n) };
        let times: Vec<u64> = if n == 0 { vec![] } else { unsafe { std::slice::from_raw_parts(p, n) }.iter().map(|x| x.to_bits()).collect() };
        t.check("stock_times", seed, f.stock_times(paper).iter().map(|x| x.to_bits()).collect::<Vec<_>>(), times);
    }
    let (mut pw, mut ph) = (0u32, 0u32);
    let ok = unsafe { sys::negs_facts_photo_pixels(h, &mut pw, &mut ph) };
    t.check("photo_pixels", seed, f.photo_pixels(), (ok == 1).then_some((pw, ph)));

    // Values, defaults and modified, on every path the lists read and a few others.
    let mut probe: Vec<String> = paths().iter().map(|(p, _)| p.to_string()).collect();
    probe.extend(["film_render.grain.rms_granularity.3".into(), "film_render.grain.uniformity.+1".into(), "io".into(), "".into(), string(r)]);
    for p in probe.iter().filter(|_| r.chance(3)) {
        let mut out = state_common::empty();
        let ok = unsafe { sys::negs_facts_value(h, p.as_ptr(), p.len(), &mut out) };
        t.check("value", seed, Some(serde_json::to_string(f.value(p)).unwrap()), json_of(out, ok));
        let mut out = state_common::empty();
        let ok = unsafe { sys::negs_facts_default_value(h, p.as_ptr(), p.len(), &mut out) };
        t.check("default_value", seed, Some(serde_json::to_string(f.default_value(p)).unwrap()), json_of(out, ok));
        t.check("is_modified", seed, f.is_modified(p), unsafe { sys::negs_facts_is_modified(h, p.as_ptr(), p.len()) } == 1);
    }

    // The edit lists.
    for (i, tab) in oe::Tab::ALL.into_iter().enumerate() {
        t.check("count", seed, oe::count(tab, f), unsafe { sys::negs_edits_count(h, i as u32) });
        let mut n = 0;
        let list = unsafe { sys::negs_edits_list(h, i as u32, decimal as u32, &mut n) };
        let core: Vec<(String, String, u32, &str)> = (0..n)
            .map(|k| {
                let e = unsafe { &*list.add(k) };
                let path = if e.path.is_null() { "" } else { unsafe { std::ffi::CStr::from_ptr(e.path) }.to_str().unwrap() };
                (unsafe { text(e.title) }, unsafe { text(e.value) }, e.target, path)
            })
            .collect();
        let oracle: Vec<(String, String, u32, &str)> = oe::edits(tab, f, decimal)
            .into_iter()
            .map(|e| {
                let (kind, path) = match e.target {
                    oe::Target::Param(p) => (0, p),
                    oe::Target::MatchBrightness => (1, ""),
                    oe::Target::Exposure => (2, ""),
                    oe::Target::ScanFilm => (3, ""),
                    oe::Target::PreviewSpace => (4, ""),
                    oe::Target::Border => (5, ""),
                    oe::Target::FilmFormat => (6, ""),
                };
                (e.title, e.value, kind, path)
            })
            .collect();
        t.check("edits", seed, oracle, core);
    }
    for i in 0..os::ADJUSTMENT_GROUPS.len() {
        t.check("group_count", seed, oe::group_count(i, f), unsafe { sys::negs_edits_group_count(h, i) });
        t.check("group_shows", seed, oe::group_shows(f, i), unsafe { sys::negs_edits_group_shows(h, i) } == 1);
        for place in 0..os::ADJUSTMENT_GROUPS[i].controls.len() {
            t.check("control_shows", seed, oe::control_shows(f, i, place), unsafe { sys::negs_edits_control_shows(h, i, place) } == 1);
        }
        let mut m: sys::negs_more_t = unsafe { std::mem::zeroed() };
        let ok = unsafe { sys::negs_edits_module_more(h, i, &mut m) };
        t.check("module_more", seed, more_o(oe::module_more(f, i)), more_c(ok, &m));
    }
    for (k, s) in SECTIONS.into_iter().enumerate() {
        t.check("section_count", seed, oe::section_count(s, f), unsafe { sys::negs_edits_section_count(h, k as u32) });
        t.check("shows_section", seed, oe::shows_section(f, s), unsafe { sys::negs_edits_shows_section(h, k as u32) } == 1);
        let mut m: sys::negs_more_t = unsafe { std::mem::zeroed() };
        let ok = unsafe { sys::negs_edits_section_more(h, k as u32, &mut m) };
        t.check("section_more", seed, more_o(oe::section_more(f, s)), more_c(ok, &m));
    }
    let others = unsafe { [sys::negs_edits_other_count(h, 0), sys::negs_edits_other_count(h, 1), sys::negs_edits_other_count(h, 2)] };
    t.check("other_count", seed, [oe::brightness_count(f), oe::output_count(f), oe::scan_film_modified(f) as usize], others);
}

fn rules_common_static(p: *const std::os::raw::c_char) -> &'static str {
    if p.is_null() { "" } else { unsafe { std::ffi::CStr::from_ptr(p) }.to_str().unwrap() }
}

fn more_o(m: Option<(String, oe::MoreAction)>) -> Option<(String, u32, String)> {
    m.map(|(text, action)| match action {
        oe::MoreAction::Reveal(k) => (text, 0, k),
        oe::MoreAction::Conceal(k) => (text, 1, k),
        oe::MoreAction::ShowFewer => (text, 2, String::new()),
    })
}

fn more_c(ok: u8, m: &sys::negs_more_t) -> Option<(String, u32, String)> {
    (ok == 1).then(|| (unsafe { text(m.text) }, m.action, unsafe { text(m.key) }))
}

/// How many cases reach each of the lists' and readers' branches.
fn cover(seen: &mut std::collections::BTreeMap<&'static str, u64>, f: &InspectorFacts) {
    let mut mark = |what: &'static str, on: bool| *seen.entry(what).or_default() += on as u64;
    for (tab, name) in oe::Tab::ALL.into_iter().zip(["Prep edits", "Lab edits", "Output edits"]) {
        mark(name, oe::count(tab, f) > 0);
    }
    let lab = oe::edits(oe::Tab::Lab, f, '.');
    let prep = oe::edits(oe::Tab::Prep, f, '.');
    for (target, name) in [
        (oe::Target::MatchBrightness, "MatchBrightness"), (oe::Target::Exposure, "Exposure"), (oe::Target::ScanFilm, "ScanFilm"),
        (oe::Target::PreviewSpace, "PreviewSpace"), (oe::Target::FilmFormat, "FilmFormat"),
    ] {
        mark(name, lab.iter().chain(&prep).any(|e| e.target == target));
    }
    mark("Color filter edit", lab.iter().any(|e| e.title == "Color filter" && e.value != "None"));
    mark("B&W grain title", f.film_is_bw() && lab.iter().any(|e| e.target.eq(&oe::Target::Param("film_render.grain.rms_granularity.0"))));
    mark("note", f.brightness.note().is_some());
    mark("note EV", f.brightness.note().is_some_and(|n| n.contains(" EV ")));
    mark("pending", f.brightness.pending());
    mark("active", f.brightness.active());
    mark("failed_here", f.brightness.failed_here());
    mark("unavailable", f.brightness.unavailable().is_some());
    mark("film_is_bw", f.film_is_bw());
    mark("stops", f.color_filters.stops(f.color_filter()).is_some());
    mark("negative", f.is_negative());
    let mores: Vec<_> = (0..os::ADJUSTMENT_GROUPS.len()).filter_map(|i| oe::module_more(f, i)).collect();
    mark("module Reveal", mores.iter().any(|m| matches!(m.1, oe::MoreAction::Reveal(_))));
    mark("module Conceal", mores.iter().any(|m| matches!(m.1, oe::MoreAction::Conceal(_))));
    let sections: Vec<_> = SECTIONS.into_iter().filter_map(|s| oe::section_more(f, s)).collect();
    mark("section Reveal", sections.iter().any(|m| matches!(m.1, oe::MoreAction::Reveal(_))));
    mark("section other", sections.iter().any(|m| !matches!(m.1, oe::MoreAction::Reveal(_))));
}

// MARK: The test

#[test]
fn edits_and_facts_match_the_frozen_rust() {
    let _guard = serial();
    let leaks = Leaks::start();
    let d = engine_defaults();
    let cat = catalog();
    let stocks: Vec<Stock> = cat["profiles"].as_array().unwrap().iter().map(Stock::new).collect();
    let filters: Vec<ColorFilter> = cat["color_filters"].as_array().unwrap().iter().map(ColorFilter::new).collect();
    let real = (stocks, filters, cat);
    let real_catalog = catalog_handle(&d.defaults, &real.0, &real.1, &real.2);
    let empty_catalog = catalog_handle(&d.defaults, &[], &[], &Value::Null);
    // One handle per catalog, updated case after case as the app's is.
    let handles = unsafe { [sys::negs_facts_new(real_catalog), sys::negs_facts_new(empty_catalog)] };
    let mut states: [*mut sys::negs_state_t; 2] = [std::ptr::null_mut(); 2];
    let mut t = Tally::default();
    let mut seen = std::collections::BTreeMap::<&str, u64>::new();
    let n = cases();
    for seed in first_seed()..first_seed() + n {
        let r = &mut Random(seed);
        let kind = match r.below(8) {
            0 => 1,
            1 => 2,
            _ => 0,
        };
        let variant = if kind == 2 {
            let stocks = stock_variant(r, &real.0);
            let filters: Vec<ColorFilter> = (0..r.index(4)).map(|_| real.1[r.index(real.1.len())].clone()).collect();
            let mut cat = real.2.clone();
            if r.bool() {
                cat = state_common::alter(r, &cat, 6);
            }
            if r.bool() && !stocks.is_empty() && cat.is_object() {
                let id = stocks[r.index(stocks.len())].id.clone();
                cat["profiles"] = json!([{"id": id, "color_filter_stops": {"hoya_x1": r.float(0.0, 3.0), "hoya_r1": state_common::scalar(r)}}]);
            }
            (stocks, filters, cat)
        } else {
            (vec![], vec![], Value::Null)
        };
        let w = world(r, &d, kind, &real, &variant);
        let f = facts(&w, &d.defaults);
        let decimal = ['.', ',', '٫', '\u{0}', '😀'][if r.chance(3) { r.index(5) } else { 0 }];

        host_mode(&mut t, seed, &f, decimal);

        // Native: the variant's catalog for this case only.
        let (h, owned) = match kind {
            2 => {
                let c = catalog_handle(&d.defaults, &w.stocks, &w.filters, &w.catalog);
                (unsafe { sys::negs_facts_new(c) }, Some(c))
            }
            k => (handles[k], None),
        };
        let slot = if kind == 2 { None } else { Some(kind) };
        let text_state = serde_json::to_vec(&w.state).unwrap();
        let state = unsafe { sys::negs_state_parse(text_state.as_ptr(), text_state.len()) };
        assert!(!state.is_null(), "seed {seed}: the core refused the state");
        let mut keep = Strings(vec![]);
        // Sometimes first another case's facts, so a stale cache would show.
        if r.chance(4) {
            let other = world(&mut Random(seed ^ 0x9e37_79b9), &d, kind, &real, &variant);
            let mut keep_other = Strings(vec![]);
            let i = input(&other, &mut keep_other);
            let t2 = serde_json::to_vec(&other.state).unwrap();
            let s2 = unsafe { sys::negs_state_parse(t2.as_ptr(), t2.len()) };
            unsafe { sys::negs_facts_update(h, s2, &i, 0) };
            let mut out = 0.0;
            let _ = unsafe { (sys::negs_facts_flags(h), sys::negs_edits_count(h, 1), sys::negs_facts_color_filter_stops(h, b"hoya_x1".as_ptr(), 7, &mut out)) };
            let mut note = sys::negs_str_t { data: std::ptr::null(), len: 0 };
            unsafe { sys::negs_facts_note(h, &mut note) };
            unsafe { sys::negs_facts_update(h, std::ptr::null(), &i, 0) };
            unsafe { sys::negs_state_free(s2) };
        }
        let borrow = r.bool();
        let i = input(&w, &mut keep);
        unsafe { sys::negs_facts_update(h, state, &i, if borrow { sys::NEGS_FACTS_BORROW } else { 0 }) };
        if !borrow {
            // Copied: the core mustn't read these again.
            for s in keep.0.iter_mut() {
                unsafe { s.as_bytes_mut() }.fill(1);
            }
        }
        native_mode(&mut t, seed, r, h, &f, &w, decimal);
        cover(&mut seen, &f);
        match slot {
            Some(k) => {
                if !states[k].is_null() {
                    unsafe { sys::negs_state_free(states[k]) };
                }
                states[k] = state;
            }
            None => unsafe {
                sys::negs_facts_free(h);
                sys::negs_catalog_free(owned.unwrap());
                sys::negs_state_free(state);
            },
        }
    }
    unsafe {
        for h in handles {
            sys::negs_facts_free(h);
        }
        for s in states {
            if !s.is_null() {
                sys::negs_state_free(s);
            }
        }
        sys::negs_catalog_free(real_catalog);
        sys::negs_catalog_free(empty_catalog);
    }
    ce::release_thread_facts();
    t.finish("edits and facts", n);
    println!("edits and facts: cases reaching {seen:?}");
    for (what, cases) in &seen {
        assert!(*cases > 0 || n < 1000, "no case reached {what}");
    }
    leaks.check("edits and facts");
}
