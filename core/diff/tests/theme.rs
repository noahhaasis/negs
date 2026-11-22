//! The themes, Odin core against the Rust oracle: windows/src/ui/theme.rs as
//! it was before the Windows app took its themes from the core, frozen in
//! tests/oracle/windows_theme.rs.txt (it links the Windows API, so it can't
//! be built here). Its tables are read from that source, and its colour
//! maths and parse are copied below, with a check that the source says the
//! same. Every theme's tokens,
//! key, title and Windows hint; luminance, contrast and mix over seeded
//! colours, bit for bit; parse over seeded and awkward names.

use negs_core_sys::theme::*;
use std::collections::HashMap;
use std::ffi::CStr;

const SOURCE: &str = include_str!("oracle/windows_theme.rs.txt");

const FIELDS: [&str; 14] =
    ["bg", "panel", "film", "grid", "line", "track", "dim", "label", "paper", "accent", "fill", "on_fill", "warn", "blank"];

fn hex_fields(h: &negs_theme_hex_t) -> [u32; 14] {
    [h.bg, h.panel, h.film, h.grid, h.line, h.track, h.dim, h.label, h.paper, h.accent, h.fill, h.on_fill, h.warn, h.blank]
}

/// The body of `const NAME: Hex = Hex { ... };` or of a `Tokens { hex: Hex { ... }` block.
fn braces_after<'a>(text: &'a str, marker: &str) -> &'a str {
    let at = text.find(marker).unwrap_or_else(|| panic!("{marker} not in theme.rs"));
    let open = at + text[at..].find('{').unwrap();
    let mut depth = 0;
    for (i, c) in text[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open + 1..open + i];
                }
            }
            _ => {}
        }
    }
    panic!("unclosed {marker}")
}

fn parse_hex(body: &str) -> HashMap<String, u32> {
    body.split(',')
        .filter_map(|part| {
            let (k, v) = part.split_once(':')?;
            let v = v.trim().strip_prefix("0x")?;
            Some((k.trim().rsplit(char::is_whitespace).next()?.to_string(), u32::from_str_radix(v, 16).ok()?))
        })
        .collect()
}

/// Windows' tables, by theme id.
fn oracle_tokens() -> Vec<([u32; 14], [u32; 14], bool, bool, bool)> {
    let table = |name: &str| -> [u32; 14] {
        let fields = parse_hex(braces_after(SOURCE, &format!("const {name}: Hex = Hex")));
        FIELDS.map(|f| *fields.get(f).unwrap_or_else(|| panic!("{name}.{f}")))
    };
    let darkroom = table("DARKROOM_HEX");
    // CONTACT_SHEET is written inline: `hex: Hex { ... }`, film Darkroom's with its own accent.
    let contact = braces_after(SOURCE, "pub const CONTACT_SHEET: Tokens");
    let contact_hex = {
        let fields = parse_hex(braces_after(contact, "hex: Hex"));
        FIELDS.map(|f| fields[f])
    };
    let contact_film = {
        let film = &contact[contact.find("film: Hex").unwrap()..];
        assert!(film.contains("..DARKROOM_HEX"), "Contact sheet's film is Darkroom's");
        let over = parse_hex(braces_after(film, "film: Hex"));
        let mut h = darkroom;
        for (i, f) in FIELDS.iter().enumerate() {
            if let Some(v) = over.get(*f) {
                h[i] = *v;
            }
        }
        h
    };
    let flags = |name: &str| -> (bool, bool, bool) {
        let line = SOURCE.lines().find(|l| l.starts_with(&format!("pub const {name}: Tokens"))).unwrap_or_else(|| panic!("{name}"));
        let line = if line.trim_end().ends_with('{') { contact } else { line };
        let flag = |f: &str| line.contains(&format!("{f}: true"));
        (flag("dark"), flag("highlighter"), flag("plain"))
    };
    let simple = |name: &str, hex: &str| {
        let h = table(hex);
        let (d, hi, p) = flags(name);
        (h, h, d, hi, p)
    };
    let (cd, ch, cp) = flags("CONTACT_SHEET");
    vec![
        simple("DARKROOM", "DARKROOM_HEX"),
        (contact_hex, contact_film, cd, ch, cp),
        simple("LIGHT_TABLE", "LIGHT_TABLE_HEX"),
        simple("DAYLIGHT", "DAYLIGHT_HEX"),
        simple("PLAIN", "PLAIN_HEX"),
        simple("PLAIN_DARK", "PLAIN_DARK_HEX"),
    ]
}

/// The strings of a `match self { Self::X => "...", ... }` in `fn name`.
fn oracle_strings(function: &str) -> Vec<String> {
    let body = braces_after(SOURCE, &format!("pub fn {function}(self) -> &'static str"));
    body.lines().filter_map(|l| l.split_once("=> \"")).map(|(_, s)| s.trim_end().trim_end_matches(',').trim_end_matches('"').to_string()).collect()
}

fn core_theme(id: u32) -> negs_theme_t {
    let mut t = negs_theme_t::default();
    assert_eq!(unsafe { negs_theme_get(id, &mut t) }, 1);
    t
}

fn text(p: *const std::os::raw::c_char) -> String {
    unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_string()
}

#[test]
fn tokens_names_and_hints_match_windows() {
    let tokens = oracle_tokens();
    let (keys, titles, hints) = (oracle_strings("key"), oracle_strings("title"), oracle_strings("hint"));
    assert_eq!(unsafe { negs_theme_count() } as usize, tokens.len());
    assert_eq!(keys.len(), 6);
    for (id, (hex, film, dark, highlighter, plain)) in tokens.iter().enumerate() {
        let t = core_theme(id as u32);
        assert_eq!(&hex_fields(&t.hex), hex, "theme {id} hex");
        assert_eq!(&hex_fields(&t.film), film, "theme {id} film");
        assert_eq!((t.dark != 0, t.highlighter != 0, t.plain != 0), (*dark, *highlighter, *plain), "theme {id} flags");
        assert_eq!(text(t.key), keys[id]);
        assert_eq!(text(t.title), titles[id]);
        assert_eq!(text(unsafe { negs_theme_hint(id as u32, NEGS_PLATFORM_WINDOWS) }), hints[id]);
    }
    assert_eq!(unsafe { negs_theme_get(6, &mut negs_theme_t::default()) }, 0);
    let mut c = negs_theme_constants_t::default();
    unsafe { negs_theme_constants(&mut c) };
    let konst = |name: &str| {
        let line = SOURCE.lines().find(|l| l.contains(&format!("const {name}:"))).unwrap_or_else(|| panic!("{name}"));
        line.split('=').nth(1).unwrap().trim().trim_end_matches(';').to_string()
    };
    assert_eq!(format!("0x{:06x}", c.highlighter_ink), konst("HIGHLIGHTER_INK").to_lowercase());
    assert_eq!(c.highlighter_opacity.to_string(), konst("HIGHLIGHTER_OPACITY"));
}

// Copied from the oracle; `the_copies_are_the_source` holds them to it.
mod oracle {
    pub fn luminance(rgb: u32) -> f64 {
        let channel = |shift: u32| {
            let c = ((rgb >> shift) & 0xff) as f64 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    pub fn contrast(a: u32, b: u32) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    pub fn mix(a: u32, b: u32, t: f64) -> u32 {
        let channel = |shift: u32| {
            let (x, y) = (((a >> shift) & 0xff) as f64, ((b >> shift) & 0xff) as f64);
            ((x + (y - x) * t).round() as u32) << shift
        };
        channel(0) | channel(8) | channel(16)
    }

    pub fn parse(text: &str, keys: &[String]) -> Option<usize> {
        let squashed: String = text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
        keys.iter().position(|k| k.to_ascii_lowercase() == squashed)
    }
}

#[test]
fn the_copies_are_the_source() {
    let this = include_str!("theme.rs");
    let squash = |s: &str| s.split_whitespace().collect::<String>();
    let source = squash(SOURCE);
    for name in ["luminance", "contrast", "mix"] {
        let copy = braces_after(this, &format!("pub fn {name}("));
        let original = braces_after(SOURCE, &format!("pub fn {name}("));
        assert_eq!(squash(copy), squash(original), "{name} drifted from the oracle");
    }
    assert!(source.contains(&squash(
        "let squashed: String = text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();"
    )));
    assert!(source.contains(&squash("Self::ALL.into_iter().find(|t| t.key().to_ascii_lowercase() == squashed)")));
}

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[test]
fn colour_maths_match_bit_for_bit() {
    let mut random = Random(0x7468_656d65);
    for _ in 0..200_000 {
        let (a, b) = ((random.next() & 0xff_ffff) as u32, (random.next() & 0xff_ffff) as u32);
        let t = (random.next() >> 11) as f64 / (1u64 << 53) as f64;
        assert_eq!(unsafe { negs_theme_luminance(a) }.to_bits(), oracle::luminance(a).to_bits(), "{a:06x}");
        assert_eq!(unsafe { negs_theme_contrast(a, b) }.to_bits(), oracle::contrast(a, b).to_bits(), "{a:06x} {b:06x}");
        assert_eq!(unsafe { negs_theme_mix(a, b, t) }, oracle::mix(a, b, t), "{a:06x} {b:06x} {t}");
    }
    for t in [0.0, 0.5, 1.0, -0.0, 0.25, 0.75, 1.0 / 3.0] {
        for (a, b) in [(0, 0xff_ffff), (0x10_2030, 0xf0_e0d0), (0x01_0101, 0x02_0202)] {
            assert_eq!(unsafe { negs_theme_mix(a, b, t) }, oracle::mix(a, b, t), "{a:06x} {b:06x} {t}");
        }
    }
}

#[test]
fn parse_matches_windows() {
    let keys = oracle_strings("key");
    let titles = oracle_strings("title");
    let core = |s: &str| match unsafe { negs_theme_parse(s.as_ptr().cast(), s.len()) } {
        -1 => None,
        id => Some(id as usize),
    };
    let mut cases: Vec<String> = keys.iter().chain(titles.iter()).cloned().collect();
    cases.extend(
        [
            "", " ", "PLAIN DARK", "plain-dark", "plain_dark!", "Plain\u{00A0}dark", "plain\tdark", "neon", "plaín", "plain2",
            "dar\u{212A}room", "Ｐlain", "plain\u{0301}", "contact sheet ✓", "light·table", "Ⅷ", "dark\u{200D}room", "ΣΑ",
        ]
        .map(String::from),
    );
    let alphabet: Vec<char> = "plaindrkoomctshegbyPLAINDRKOMCTSHEGBY -_·.1\u{00A0}é\u{212A}".chars().collect();
    let mut random = Random(0x7061_7273);
    for _ in 0..50_000 {
        let base = if random.next() % 2 == 0 { &keys } else { &titles };
        let mut s: Vec<char> = base[(random.next() % 6) as usize].chars().collect();
        for _ in 0..(random.next() % 3) {
            let at = (random.next() as usize) % (s.len() + 1);
            match random.next() % 3 {
                0 => s.insert(at, alphabet[(random.next() as usize) % alphabet.len()]),
                1 if at < s.len() => {
                    s.remove(at);
                }
                _ => {
                    if at < s.len() {
                        s[at] = s[at].to_ascii_uppercase();
                    }
                }
            }
        }
        cases.push(s.into_iter().collect());
    }
    let mut matched = 0;
    for case in &cases {
        let want = oracle::parse(case, &keys);
        assert_eq!(core(case), want, "{case:?}");
        matched += want.is_some() as usize;
    }
    assert!(matched > cases.len() / 4, "{matched} of {} matched", cases.len());
}
