//! The border, Odin core (negs_core_rules::border) against the Rust oracle
//! (windows/src/border.rs as it was, core/diff/oracle/border.rs), and the
//! core's ports of the engine's sizes against spektrafilm-io's `margin`,
//! `white_margin` (through `instagram_canvas`), `keyline_width`,
//! `film_geometry`, `instagram_canvas`, `FilmFormat::of` and the 35mm
//! `Strip`'s `turned` and `canvas` for every film it knows and some it
//! doesn't. Seeded inputs (NEGS_RULES_SEEDS, default 10,000;
//! NEGS_RULES_SEED), every output compared exactly; Rust's `{}` of an f64
//! (the exporter's arguments) on wild values too; the serde format (session
//! files and sidecars) byte for byte both ways.

mod picture_common;

use negs_core_diff_picture::border as oracle;
use negs_core_rules::border as core;
use negs_core_sys::picture as sys;
use picture_common::{Random, Tally, cases, gate, seed};
use spektrafilm_io::border::{BorderStyle, FilmFormat};
use spektrafilm_io::film35::{STOCKS, Strip};

fn style_id(s: BorderStyle) -> u32 {
    core::STYLES.iter().position(|x| *x == s).unwrap() as u32
}
fn format_id(f: FilmFormat) -> u32 {
    match f {
        FilmFormat::Full => 0,
        FilmFormat::Half => 1,
    }
}

fn o_settings(s: &core::BorderSettings) -> oracle::BorderSettings {
    oracle::BorderSettings {
        enabled: s.enabled,
        style: s.style,
        export: s.export,
        instagram: s.instagram,
        width: s.width,
        film_width: s.film_width,
        line: s.line,
        color: oracle::BorderColor(s.color.0),
        line_color: oracle::BorderColor(s.line_color.0),
    }
}

fn film(r: &mut Random) -> String {
    match r.below(8) {
        0 => String::new(),
        1 => "not_a_film".into(),
        2 => "kodak_portra_400 ".into(),
        _ => STOCKS[r.below(STOCKS.len() as u64) as usize].0.to_string(),
    }
}

/// A photograph's pixel size: mostly near the 35mm frames' 3:2 and 4:3 (the
/// 1% edge on both sides), and any.
fn photo(r: &mut Random) -> (u32, u32) {
    let (w, h) = match r.below(6) {
        0 | 1 => {
            let long = 1 + r.below(9000) as u32;
            let ratio = if r.chance(2) { 1.5 } else { 4.0 / 3.0 } * r.range(0.985, 1.015);
            (long, ((long as f64) / ratio).round() as u32)
        }
        2 => (r.pixels(), r.pixels()),
        _ => (1 + r.below(9000) as u32, 1 + r.below(9000) as u32),
    };
    if r.chance(2) { (w, h) } else { (h, w) }
}

fn fraction(r: &mut Random) -> f64 {
    match r.below(8) {
        0 => r.wild(),
        1 => r.range(-0.5, 0.0),
        2 => r.range(0.0, 1.0),
        3 => (r.below(500) as f64) / 1000.0,
        _ => r.range(0.0, 0.25),
    }
}

fn color(r: &mut Random) -> core::BorderColor {
    core::BorderColor([r.below(256) as u8, r.below(256) as u8, r.below(256) as u8])
}

fn settings(r: &mut Random) -> core::BorderSettings {
    core::BorderSettings {
        enabled: !r.chance(4),
        style: core::STYLES[r.below(4) as usize],
        export: !r.chance(4),
        instagram: r.chance(3),
        width: fraction(r),
        film_width: fraction(r),
        line: fraction(r),
        color: color(r),
        line_color: color(r),
    }
}

fn hex_text(r: &mut Random) -> String {
    let c = color(r);
    let plain = format!("{:02x}{:02x}{:02x}", c.0[0], c.0[1], c.0[2]);
    match r.below(12) {
        0 => plain,
        1 => format!("#{}", plain.to_uppercase()),
        2 => format!("#{}", &plain[..r.below(6) as usize]),
        3 => format!("#{plain}0"),
        4 => format!("##{plain}"),
        5 => format!(" #{plain}"),
        6 => "#12345g".into(),
        7 => "#ééé".into(),
        8 => "#+1+2+3".into(),
        9 => String::new(),
        10 => "#".into(),
        _ => format!("#{plain}"),
    }
}

/// Values for Rust's `{}`: ordinary fractions, round ones, and the edges.
fn number(r: &mut Random) -> f64 {
    match r.below(10) {
        0 => r.wild(),
        1 => f64::from_bits(r.next()),
        2 => (r.int(-100_000, 100_000) as f64) / 1000.0,
        3 => r.range(-1e30, 1e30),
        4 => r.range(-1e-30, 1e-30),
        5 => 10f64.powi(r.int(-320, 308) as i32) * if r.chance(2) { 1.0 } else { -1.0 },
        6 => (r.next() >> 11) as f64,
        _ => r.range(0.0, 0.5),
    }
}

fn case(r: &mut Random, t: &mut Tally, i: usize) {
    let w = |what: &'static str| move || format!("case {i}: {what}");
    let (pw, ph) = photo(r);
    let f = fraction(r);

    // The engine's sizes, ported.
    t.eq(&w("margin"), spektrafilm_io::border::margin(pw, ph, f), unsafe { sys::negs_border_margin(pw, ph, f) });
    t.eq(&w("keyline_width"), spektrafilm_io::border::keyline_width(pw, ph, f), unsafe { sys::negs_border_keyline_width(pw, ph, f) });
    let mut out = [0u32; 2];
    unsafe { sys::negs_border_film_geometry(pw, ph, &mut out) };
    let e = spektrafilm_io::border::film_geometry(pw, ph);
    t.eq(&w("film_geometry"), [e.0, e.1], out);
    let fmt = FilmFormat::of(pw, ph);
    let core_fmt = unsafe { sys::negs_film_format_of(pw, ph) };
    t.eq(&w("FilmFormat::of"), fmt.map(format_id).map(|x| x as i32).unwrap_or(-1), core_fmt);
    for style in core::STYLES {
        // The canvas a border makes can be any size: the photograph's and more.
        let (cw, ch) = if r.chance(3) { (pw, ph) } else { (r.pixels() / if r.chance(2) { 1 } else { 4 }, r.pixels() / if r.chance(2) { 1 } else { 4 }) };
        unsafe { sys::negs_border_instagram_canvas(cw, ch, style_id(style), &mut out) };
        let e = spektrafilm_io::border::instagram_canvas(cw, ch, style);
        t.eq(&w("instagram_canvas"), [e.0, e.1], out);
    }
    for format in [FilmFormat::Full, FilmFormat::Half] {
        let strip = Strip::new(&film(r), format);
        t.eq(&w("Strip::turned"), strip.turned(pw, ph), unsafe { sys::negs_strip_turned(format_id(format), pw, ph) } != 0);
        unsafe { sys::negs_strip_canvas(format_id(format), pw, ph, &mut out) };
        let e = strip.canvas(pw, ph);
        t.eq(&w("Strip::canvas"), [e.0, e.1], out);
        t.eq(&w("format name"), format.name(), core::FilmFormat::name(format));
    }

    // The app's rules.
    for style in core::STYLES {
        t.eq(&w("title"), oracle::title(style), core::title(style));
        t.eq(&w("help"), oracle::help(style), core::help(style));
        t.eq(&w("raw_value"), oracle::raw_value(style), core::raw_value(style));
        t.eq(&w("fits"), oracle::fits(style, pw, ph), core::fits(style, pw, ph));
    }
    let s = settings(r);
    let os = o_settings(&s);
    let fm = film(r);
    t.dbg(&w("shown"), os.shown(pw, ph), s.shown(pw, ph).map(|b| o_settings(&b)));
    t.dbg(&w("exported"), os.exported(pw, ph), s.exported(pw, ph).map(|b| o_settings(&b)));
    t.dbg(&w("style_width"), os.style_width(), s.style_width());
    t.eq(&w("keyline"), os.keyline(pw, ph), s.keyline(pw, ph));
    t.eq(&w("canvas"), os.canvas(pw, ph, &fm), s.canvas(pw, ph, &fm));
    let canvas = (r.pixels() / 2, r.pixels() / 2);
    t.eq(&w("instagram_canvas"), os.instagram_canvas(canvas), s.instagram_canvas(canvas));
    t.eq(&w("document"), os.document(pw, ph, &fm), s.document(pw, ph, &fm));
    t.eq(&w("options"), os.options().to_string(), s.options().to_string());
    t.eq(&w("arguments"), os.arguments(), s.arguments());
    t.eq(&w("strip_turned"), oracle::strip_turned(&fm, pw, ph), core::strip_turned(&fm, pw, ph));

    // Colours.
    let hex = hex_text(r);
    t.eq(&w("from_hex"), oracle::BorderColor::from_hex(&hex).map(|c| c.0), core::BorderColor::from_hex(&hex).map(|c| c.0));
    let c = color(r);
    let oc = oracle::BorderColor(c.0);
    t.eq(&w("hex"), oc.hex(), c.hex());
    t.dbg(&w("rgba"), oc.rgba(), c.rgba());
    t.eq(&w("colorref"), oc.colorref(), c.colorref());
    let cr = r.next() as u32;
    t.eq(&w("from_colorref"), oracle::BorderColor::from_colorref(cr).0, core::BorderColor::from_colorref(cr).0);

    // Rust's `{}`, as the exporter's arguments write numbers.
    let v = number(r);
    t.eq(&w("display"), format!("{v}"), core::rust_display(v));

    // The serde format.
    let (a, b) = (serde_json::to_string(&os).unwrap(), serde_json::to_string(&s).unwrap());
    t.eq(&w("serde write"), a.clone(), b);
    let back_o: Result<oracle::BorderSettings, _> = serde_json::from_str(&a);
    let back_c: Result<core::BorderSettings, _> = serde_json::from_str(&a);
    t.dbg(&w("serde read"), back_o.ok(), back_c.ok().map(|c| o_settings(&c)));
    let parts = [
        format!(r#""enabled":{}"#, r.chance(2)),
        format!(r#""style":"{}""#, ["solid", "film", "keyline", "film35", "35mm", "Solid", "bogus"][r.below(7) as usize]),
        format!(r#""export":{}"#, if r.chance(5) { "1".to_string() } else { r.chance(2).to_string() }),
        format!(r#""instagram":{}"#, r.chance(2)),
        format!(r#""width":{}"#, if r.chance(6) { "null".to_string() } else { format!("{:?}", r.range(0.0, 0.2)) }),
        format!(r#""filmWidth":{:?}"#, r.range(0.0, 0.2)),
        format!(r#""line":{}"#, r.below(3)),
        format!(r#""color":"{}""#, hex_text(r)),
        format!(r#""lineColor":"{}""#, hex_text(r)),
        r#""film_width":0.5"#.to_string(),
        r#""extra":[1,2]"#.to_string(),
    ];
    let chosen: Vec<&str> = parts.iter().filter(|_| r.chance(2)).map(|s| s.as_str()).collect();
    let text = format!("{{{}}}", chosen.join(","));
    let o: Result<oracle::BorderSettings, _> = serde_json::from_str(&text);
    let c: Result<core::BorderSettings, _> = serde_json::from_str(&text);
    t.eq(&w("odd read ok"), o.as_ref().map_err(|e| e.to_string()).err(), c.as_ref().map_err(|e| e.to_string()).err());
    if let (Ok(o), Ok(c)) = (o, c) {
        t.dbg(&w("odd read"), o, o_settings(&c));
        t.eq(&w("odd rewrite"), serde_json::to_string(&o).unwrap(), serde_json::to_string(&c).unwrap());
    }
}

#[test]
fn border_matches_the_rust_and_the_engine() {
    let seed = seed(0x3b_b0de);
    let n = cases(10_000);
    gate("border", seed, |t| {
        let mut r = Random(seed);
        for i in 0..n {
            case(&mut r, t, i);
        }
        // Every film the 35mm strip knows, at both formats and orientations.
        for (film, ..) in STOCKS {
            for (pw, ph) in [(6000, 4000), (4000, 6000), (4000, 3000), (3000, 4000), (1, 1), (3, 2)] {
                for format in [FilmFormat::Full, FilmFormat::Half] {
                    let mut out = [0u32; 2];
                    unsafe { sys::negs_strip_canvas(format_id(format), pw, ph, &mut out) };
                    let e = Strip::new(film, format).canvas(pw, ph);
                    t.eq(&|| format!("{film} {pw}x{ph}"), [e.0, e.1], out);
                }
                t.eq(&|| format!("{film} strip_turned"), oracle::strip_turned(film, pw, ph), core::strip_turned(film, pw, ph));
            }
        }
        n
    });
}
