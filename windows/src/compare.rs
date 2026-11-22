//! Compare (ticket 46, `Compare.swift`): the rules and geometry, from the
//! shared Odin core (core/src/rules/compare) through negs-core-rules,
//! which keeps this module's API. The tests below are the module's own,
//! run against the core (also on any OS: windows/library-map, and the
//! wrapper); core/diff holds the core to the Rust this was
//! (core/diff/oracle/compare.rs).

pub use negs_core_rules::compare::*;

#[cfg(test)]
use negs_library::Item;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn item(name: &str) -> Item {
        Item::photo(Path::new(name))
    }

    fn number(i: &Item) -> String {
        i.photo.to_string_lossy().trim_start_matches('p').to_string()
    }

    #[test]
    fn the_status_line_says_what_differs() {
        let facts = |iso: &str, shutter: &str, aperture: &str, focal: Option<&str>| {
            [Some("Nikon D750".to_string()), Some(iso.to_string()), Some(shutter.to_string()), Some(aperture.to_string()), focal.map(String::from)]
        };
        let a = facts("ISO 400", "1/160", "f/4", None);
        let b = facts("ISO 400", "1/250", "f/5.6", None);
        assert_eq!(differences(Some(&a), Some(&b), ("Portra", "Portra"), (Some("Endura"), Some("Endura"))), "shutter and aperture");
        assert_eq!(differences(Some(&a), Some(&a), ("Portra", "Gold"), (Some("Endura"), None)), "film and paper");
        let c = facts("ISO 800", "1/250", "f/5.6", None);
        assert_eq!(differences(Some(&a), Some(&c), ("Portra", "Gold"), (None, None)), "ISO, shutter, aperture and film");
        // A part only one has differs; unknown facts don't, yet.
        let d = facts("ISO 400", "1/160", "f/4", Some("50 mm"));
        assert_eq!(differences(Some(&a), Some(&d), ("P", "P"), (None, None)), "focal length");
        assert_eq!(differences(Some(&a), None, ("P", "P"), (None, None)), "");
    }

    #[test]
    fn compare_is_offered_for_two_never_three() {
        let (a, b, c) = (item("p3"), item("p7"), item("p9"));
        let three = offer(&[a.clone(), b.clone(), c.clone()], Some(&a), &a, &number, true, true, "Original");
        assert_eq!((three.title.as_str(), three.enabled, three.pair.is_none()), ("Compare Two Photographs", false, true));
        let two = offer(&[a.clone(), b.clone()], Some(&a), &a, &number, true, true, "Original");
        assert_eq!((two.title.as_str(), two.enabled), ("Compare 3 and 7", true));
        assert_eq!(two.pair, Some((a.clone(), b.clone())));
        assert!(!offer(&[a.clone(), b.clone()], None, &a, &number, false, true, "Original").enabled, "while cropping, say");
        // One: with the current photograph, or itself with its original.
        let with = offer(&[b.clone()], Some(&a), &b, &number, true, true, "Original");
        assert_eq!((with.title.as_str(), with.pair), ("Compare with 3", Some((a.clone(), b.clone()))));
        let original = offer(&[a.clone()], Some(&a), &a, &number, true, true, "Camera JPEG");
        assert_eq!((original.title.as_str(), original.enabled, original.with_original), ("Compare with Camera JPEG", true, true));
        assert!(!offer(&[a.clone()], Some(&a), &a, &number, true, false, "Original").enabled, "in the grid, say");
    }

    #[test]
    fn the_compare_word_acts_on_two_selected() {
        let (a, b, c) = (item("p3"), item("p7"), item("p9"));
        assert_eq!(word(&[a.clone(), b.clone()], &number, true), (true, "Compare 3 and 7 (C)".into()));
        assert!(!word(&[a.clone(), b.clone()], &number, false).0);
        assert_eq!(word(&[a.clone()], &number, true), (false, "Select a second photograph to compare (Ctrl-click)".into()));
        assert_eq!(word(&[], &number, true).0, false);
        assert_eq!(word(&[a, b, c], &number, true), (false, "Compare takes two photographs. Three or more can't be compared.".into()));
        assert_eq!((menu_title(false), menu_title(true)), ("C&ompare\tC", "Stop C&omparing\tC"));
    }

    #[test]
    fn steps_skip_the_first_and_stop_at_the_ends() {
        let order: Vec<Item> = ["p1", "p2", "p3", "p4"].iter().map(|n| item(n)).collect();
        assert_eq!(step(&order[1], &order[2], &order, 1), Some(order[3].clone()), "past the first");
        assert_eq!(step(&order[3], &order[0], &order, 1), None, "the end stops it");
        assert_eq!(step(&order[1], &order[0], &order, -1), None);
        // The second is gone from view: steps from where the first is.
        assert_eq!(step(&item("p9"), &order[1], &order, 1), Some(order[2].clone()));
        assert_eq!(step(&item("p9"), &order[1], &order, -1), Some(order[0].clone()));
    }

    #[test]
    fn frame_numbers_count_photographs_and_letter_duplicates() {
        let dup = |name: &str, v: &str| Item { photo: Path::new(name).into(), variant: Some(v.into()) };
        let items = vec![item("a"), dup("b", "x"), dup("b", "y"), item("c")];
        assert_eq!(frame_number(&items, &items[0]), "1");
        assert_eq!(frame_number(&items, &items[1]), "2a");
        assert_eq!(frame_number(&items, &items[2]), "2b");
        assert_eq!(frame_number(&items, &items[3]), "3");
        assert_eq!(frame_number(&items, &item("z")), "");
    }

    #[test]
    fn two_landscapes_stack_in_a_tall_window() {
        let (arrangement, frames) = layout((900.0, 1200.0), (1.5, 1.5));
        assert_eq!(arrangement, Arrangement::Column);
        assert!(frames[0][1] < frames[1][1]);
        let (arrangement, frames) = layout((1600.0, 900.0), (1.5, 1.5));
        assert_eq!(arrangement, Arrangement::Row);
        assert_eq!(frames[0][2], frames[1][2]);
        assert!(frames[0][0] + frames[0][2] + GAP <= frames[1][0] + 1.0);
        let slider = slider_frame((1600.0, 900.0), 1.5);
        assert_eq!(slider[1], (CAPTION + (900.0 - CAPTION - LABEL - slider[3]) / 2.0).round());
    }

    #[test]
    fn original_modes_keep_the_macs_keys() {
        for mode in OriginalMode::ALL {
            assert_eq!(OriginalMode::from_key(mode.key()), mode);
        }
        assert_eq!(OriginalMode::from_key("nonsense"), OriginalMode::Hold);
        assert_eq!(OriginalReference::from_key("cameraJPEG"), OriginalReference::CameraJpeg);
        assert_eq!(OriginalMode::Split.help("camera JPEG"), "Show the camera JPEG left of a line you can drag. Hold \\ to see the whole camera JPEG.");
    }
}
