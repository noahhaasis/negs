//! The film strip's and the grid's rules (`StripLayout`, `SheetLayout` in
//! `ScrollWatch.swift` and `Selection.swift`; `EditorModel.thumbnailCentre`,
//! `outward`, `prefetchTargets`, `pumpThumbnails`; `SheetWords`), from the
//! shared Odin core (core/src/rules/sheet) through negs-core-rules, which
//! keeps this module's API; the bounded memory cache (`Lru`) is Rust there.
//! The tests below are the module's own, run against the core; the
//! wrapper runs them on any OS too, and core/diff holds the core to the
//! Rust this was (core/diff/oracle/sheet_rules.rs).

pub use negs_core_rules::sheet_rules::*;

#[cfg(test)]
use negs_library::Filter;

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
    }

    #[test]
    fn a_thumbnail_keeps_the_known_shape_unless_it_really_differs() {
        // 6016 × 4016 developed at 512 × 342, or trimmed to 512 × 340.
        let header = 6016.0 / 4016.0;
        assert_eq!(kept_ratio(Some(header), 512.0 / 342.0), header);
        assert_eq!(kept_ratio(Some(header), 512.0 / 340.0), header);
        // Cropped square, or turned: the thumbnail's.
        assert_eq!(kept_ratio(Some(header), 1.0), 1.0);
        assert_eq!(kept_ratio(Some(header), 342.0 / 512.0), 342.0 / 512.0);
        assert_eq!(kept_ratio(None, 512.0 / 342.0), 512.0 / 342.0);
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
        // That touches don't grow its queue (a private field now) is
        // checked in negs-core-rules, where the cache lives.
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
    fn the_fade_runs_from_nothing_to_the_sheet() {
        assert_eq!(sheet_look(0.0), (0.0, FADE_RISE));
        assert_eq!(sheet_look(FADE_MS), (1.0, 0.0));
        assert_eq!(sheet_look(FADE_MS * 2.0), (1.0, 0.0));
        // Quick off the mark, as cubic-bezier(.22,.8,.24,1).
        assert!(sheet_look(FADE_MS / 4.0).1 < FADE_RISE * 0.4);
        assert!((cubic_bezier(0.25, 0.25, 0.75, 0.75, 0.3) - 0.3).abs() < 1e-6, "linear stays linear");
        // The compositor's cubics follow the look closely.
        let length = FADE_MS / 1000.0;
        for channel in [0, 1] {
            let value = |t: f64| if channel == 0 { sheet_look(t * 1000.0).0 } else { sheet_look(t * 1000.0).1 };
            let pieces = cubic_pieces(value, length, 15);
            for k in 0..=200 {
                let t = length * k as f64 / 200.0;
                let p = pieces.iter().rev().find(|p| p[0] <= t).unwrap();
                let u = t - p[0];
                let at = p[1] + p[2] * u + p[3] * u * u + p[4] * u * u * u;
                assert!((at - value(t)).abs() < 0.02, "channel {channel} at {t}: {at} vs {}", value(t));
            }
        }
    }

    #[test]
    fn rects_span() {
        let r = Rect::spanning((10.0, 20.0), (0.0, 5.0));
        assert_eq!(r, Rect::new(0.0, 5.0, 10.0, 15.0));
        assert!(r.intersects(&Rect::new(9.0, 19.0, 5.0, 5.0)));
        assert!(!r.intersects(&Rect::new(10.0, 5.0, 5.0, 5.0)), "touching edges don't count");
    }
}
