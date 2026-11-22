//! The tooltips of the inspector's controls, keyed by parameter path, and of
//! its groups, keyed by title: the same texts as `ControlHelp.swift`, which a
//! test compares this table with. A control without an entry has no tooltip.
//!
//! The tables live in the shared core (core/src/rules/help, through
//! `negs_core_rules`); this module passes it the backlight's path and the
//! grain's per-layer fields, which are this app's.

pub use negs_core_rules::help::{FILM, PAPER, PAPER_KEPT, PAPER_LOCK, group};
/// Prep › Backlight, on a negative (`NegativeScanRows`): what `text` gives
/// for `negative::SCAN_ILLUMINANT`. Kept for the API; nothing else names it.
#[allow(unused_imports)]
pub use negs_core_rules::help::SCAN_ILLUMINANT;

/// A control's tooltip. A grain lane (`film_render.grain.rms_granularity.1`)
/// shares its array's.
pub fn text(path: &str) -> Option<&'static str> {
    negs_core_rules::help::text(path, crate::negative::SCAN_ILLUMINANT, &crate::state::GRAIN_STOCK_FIELDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ADJUSTMENT_GROUPS, Adjustment, PREP_COLOR, PREP_DETAIL, PREP_TONE};

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

    /// The same texts as the macOS app, so the two can't drift.
    #[test]
    fn the_texts_are_the_macs() {
        let swift = include_str!("../../macos/Sources/NegsApp/ControlHelp.swift");
        let own = |table: &[(&str, &str)]| table.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>();
        assert_eq!(swift_pairs(swift, "paths"), own(&negs_core_rules::help::paths()));
        assert_eq!(swift_pairs(swift, "groups"), own(&negs_core_rules::help::groups()));
        // The Mac keeps the paper with a lock by "printed on"; here it's
        // still the Settings toggle, so only the clause before "unless" is shared.
        let paper = PAPER.split(", unless").next().unwrap();
        assert!(swift.contains(&format!("static let film = \"{FILM}\"")) && swift.contains(&format!("static let paper = \"{paper}, unless ")));
        assert!(swift.contains(&format!("static let paperLock = \"{PAPER_LOCK}\"")) && swift.contains(&format!("static let paperKept = \"{PAPER_KEPT}\"")));
    }

    /// Every inspector control and group explains itself (`ControlHelpTests`).
    #[test]
    fn every_control_and_group_has_a_tooltip() {
        let controls = ADJUSTMENT_GROUPS.iter().flat_map(|g| g.controls.iter()).chain(PREP_TONE).chain(PREP_COLOR).chain(PREP_DETAIL);
        let mut missing: Vec<&str> = vec![];
        for control in controls {
            match control {
                Adjustment::BalancePrint { help, .. } | Adjustment::FilterLink { help } => assert!(!help.is_empty()),
                other if text(other.path()).is_none() => missing.push(other.path()),
                _ => {}
            }
        }
        for path in [
            "io.raw_white_balance.mode", "io.raw_white_balance.temperature", "io.raw_white_balance.tint", "camera.auto_exposure_method",
            "camera.exposure_compensation_ev", "camera.film_format_mm", "camera.lens_blur_um", "io.input_color_space",
            "io.input_cctf_decoding", "io.input_interpretation.linear",
        ] {
            if text(path).is_none() {
                missing.push(path);
            }
        }
        assert_eq!(missing, Vec::<&str>::new(), "Controls without a tooltip");
        for title in ADJUSTMENT_GROUPS.iter().map(|g| g.title).chain(["Input Colour", "Output"]) {
            assert!(group(title).is_some(), "{title}");
        }
        assert_eq!(text("film_render.grain.uniformity.2"), text("film_render.grain.uniformity"));
    }
}
