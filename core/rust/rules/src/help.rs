//! The tooltips of the inspector's controls, keyed by parameter path, and of
//! its groups, keyed by title (windows/src/help.rs's tables, now in
//! core/src/rules/help): the same texts as `ControlHelp.swift`, which a test
//! compares them with. A control without an entry has no tooltip.
//!
//! The backlight's path (`negative::SCAN_ILLUMINANT`) and the grain fields
//! with a lane per layer (`state::GRAIN_STOCK_FIELDS`) belong to modules the
//! core doesn't have yet, so `text` takes them; windows/src/help.rs passes
//! its own and keeps `text(path)`.

use negs_core_sys::media as sys;

/// Prep › Backlight, on a negative (`NegativeScanRows`; not in the Mac's
/// `ControlHelp` table).
pub const SCAN_ILLUMINANT: &str = "The light behind the negative while it was photographed or scanned. Daylight suits most light pads and scanners.";
pub const FILM: &str = "The film the photograph is shot on: its colour response, contrast, grain and halation, from the stock’s measured data.";
pub const PAPER: &str = "The paper the negative is printed on: its contrast, colour and blacks. Changing the film picks the paper it was made for, unless the lock on “printed on”, under the photograph, is closed.";
/// The paper lock, under the photograph and in the paper tray's foot: open, then closed.
pub const PAPER_LOCK: &str = "Keep this paper when you change film. Open, a new film brings the paper made for it.";
pub const PAPER_KEPT: &str = "This paper stays when you change film. Click to let a new film bring the paper made for it.";

fn static_str(s: sys::negs_help_str_t) -> &'static str {
    // The core's tables: static UTF-8 for the life of the process.
    let bytes: &'static [u8] = if s.data.is_null() { &[] } else { unsafe { std::slice::from_raw_parts(s.data, s.len) } };
    std::str::from_utf8(bytes).expect("UTF-8")
}

fn help_str(s: &str) -> sys::negs_help_str_t {
    sys::negs_help_str_t { data: s.as_ptr(), len: s.len() }
}

/// A control's tooltip. A grain lane (`film_render.grain.rms_granularity.1`)
/// shares its array's. `scan_illuminant` is the backlight's path, and
/// `grain_fields` the grain's per-layer fields (at most 16).
pub fn text(path: &str, scan_illuminant: &str, grain_fields: &[&str]) -> Option<&'static str> {
    assert!(grain_fields.len() <= 16, "at most 16 grain fields");
    let fields: Vec<sys::negs_help_str_t> = grain_fields.iter().map(|f| help_str(f)).collect();
    let mut out = sys::negs_help_str_t::default();
    let found = unsafe { sys::negs_help_text(path.as_ptr(), path.len(), scan_illuminant.as_ptr(), scan_illuminant.len(), fields.as_ptr(), fields.len(), &mut out) };
    (found != 0).then(|| static_str(out))
}

pub fn group(title: &str) -> Option<&'static str> {
    let mut out = sys::negs_help_str_t::default();
    (unsafe { sys::negs_help_group(title.as_ptr(), title.len(), &mut out) } != 0).then(|| static_str(out))
}

fn rows(table: u32) -> Vec<(&'static str, &'static str)> {
    let mut out = vec![];
    loop {
        let (mut key, mut text) = (sys::negs_help_str_t::default(), sys::negs_help_str_t::default());
        if unsafe { sys::negs_help_row(table, out.len(), &mut key, &mut text) } == 0 {
            return out;
        }
        out.push((static_str(key), static_str(text)));
    }
}

/// The controls' tooltips by path, in the table's order.
pub fn paths() -> Vec<(&'static str, &'static str)> {
    rows(0)
}

/// The groups' tooltips by title, in the table's order.
pub fn groups() -> Vec<(&'static str, &'static str)> {
    rows(1)
}

/// The core's copies of the constants above, in order: SCAN_ILLUMINANT,
/// FILM, PAPER, PAPER_LOCK, PAPER_KEPT.
pub fn core_constants() -> [&'static str; 5] {
    [0, 1, 2, 3, 4].map(|i| {
        let mut out = sys::negs_help_str_t::default();
        assert!(unsafe { sys::negs_help_constant(i, &mut out) } != 0);
        static_str(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCAN: &str = "film_render.convert.scan_illuminant";
    const GRAIN: [&str; 4] = ["rms_granularity", "density_min", "uniformity", "particle_scale_sublayers"];

    #[test]
    fn the_constants_are_the_cores() {
        assert_eq!(core_constants(), [SCAN_ILLUMINANT, FILM, PAPER, PAPER_LOCK, PAPER_KEPT]);
    }

    /// The `"key": "text"` pairs of a Swift dictionary literal.
    fn swift_pairs(source: &str, name: &str) -> Vec<(String, String)> {
        let start = source.find(&format!("static let {name}")).unwrap();
        let body = &source[start..source[start..].find("\n    ]").unwrap() + start];
        body.lines()
            .filter_map(|line| {
                let line = line.trim().trim_end_matches(',');
                let (key, text) = line.strip_prefix('"')?.split_once("\": \"")?;
                Some((key.to_string(), text.strip_suffix('"')?.to_string()))
            })
            .collect()
    }

    /// The same texts as the macOS app (windows/src/help.rs's own test).
    #[test]
    fn the_texts_are_the_macs() {
        let swift = include_str!("../../../../macos/Sources/NegsApp/ControlHelp.swift");
        let own = |table: Vec<(&str, &str)>| table.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>();
        assert_eq!(swift_pairs(swift, "paths"), own(paths()));
        assert_eq!(swift_pairs(swift, "groups"), own(groups()));
        let paper = PAPER.split(", unless").next().unwrap();
        assert!(swift.contains(&format!("static let film = \"{FILM}\"")) && swift.contains(&format!("static let paper = \"{paper}, unless ")));
        assert!(swift.contains(&format!("static let paperLock = \"{PAPER_LOCK}\"")) && swift.contains(&format!("static let paperKept = \"{PAPER_KEPT}\"")));
    }

    #[test]
    fn lanes_share_their_arrays_tooltip() {
        assert_eq!(text("film_render.grain.uniformity.2", SCAN, &GRAIN), text("film_render.grain.uniformity", SCAN, &GRAIN));
        assert!(text("film_render.grain.uniformity.2", SCAN, &GRAIN).is_some());
        assert_eq!(text(SCAN, SCAN, &GRAIN), Some(SCAN_ILLUMINANT));
        assert_eq!(text("film_render.grain.uniformity.2", SCAN, &[]), None);
        assert!(group("Grain").is_some() && group("grain").is_none());
    }
}
