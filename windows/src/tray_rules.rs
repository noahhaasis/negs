//! The film and paper tray's rules (IMPLEMENTATION.md §4.5, §5.1, §5.2;
//! `Tray.swift`'s `StockTrayRules` and `TrayVisit`, and the keys of
//! `EditorWindowController.trayKeys`), from the shared Odin core
//! (core/src/rules/tray) through negs-core-rules, which keeps this
//! module's API. The tests below are the module's own, run against the
//! core (also on any OS: windows/tray-rules, and the wrapper); core/diff
//! holds the core to the Rust this was (core/diff/oracle/tray_rules.rs).

pub use negs_core_rules::tray_rules::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn group(title: &str, ids: &[&str]) -> Section {
        Section::group(title, ids.iter().map(|id| Entry::new(*id, id.to_uppercase())).collect())
    }

    fn entries(ids: &[&str]) -> Vec<Entry> {
        ids.iter().map(|id| Entry::new(*id, id.to_uppercase())).collect()
    }

    /// The Mac's `StockGroup.matching`, for the tests: every word in the
    /// name, id or group.
    fn matching(query: &str, groups: &[Section]) -> Vec<Section> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        groups
            .iter()
            .map(|g| Section {
                stocks: g.stocks.iter().filter(|e| words.iter().all(|w| format!("{} {} {}", e.name, e.id, g.title).to_lowercase().contains(w))).cloned().collect(),
                ..g.clone()
            })
            .collect()
    }

    fn row() -> Vec<Section> {
        sections(vec![group("Color Negative", &["gold", "portra", "ektar"]), group("Black & White", &["hp5", "trix"])], entries(&["portra", "trix"]), "")
    }

    #[test]
    fn a_group_glides_there_eased_in_a_quarter_second() {
        assert_eq!(glide(100.0, 500.0, 0.0), 100.0);
        assert_eq!(glide(100.0, 500.0, GLIDE_MS / 2.0), 300.0);
        assert!(glide(100.0, 500.0, GLIDE_MS / 10.0) < 100.0 + 400.0 / 10.0, "it starts slowly");
        assert_eq!(glide(100.0, 500.0, GLIDE_MS), 500.0);
        assert_eq!(glide(100.0, 500.0, 10.0 * GLIDE_MS), 500.0);
    }

    #[test]
    fn screen_readers_hear_the_macs_words() {
        assert_eq!(star_label("Portra 400", true), "Remove Portra 400 from favourites");
        assert_eq!(star_label("Portra 400", false), "Add Portra 400 to favourites");
        assert_eq!(tile_value(true, false), "Trying");
        assert_eq!(tile_value(false, true), "Before");
        assert_eq!(tile_value(true, true), "");
        assert_eq!(tile_value(false, false), "");
        assert_eq!(index_label("Black & White"), "Show Black & White");
    }

    #[test]
    fn favourites_lead_under_their_own_title_and_stay_in_their_groups() {
        let row = row();
        assert_eq!(row.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Favourites", "Color Negative", "Black & White"]);
        assert!(row[0].favourites && !row[1].favourites);
        assert_eq!(places(&row).iter().filter(|p| p.stock == "portra").count(), 2);
    }

    #[test]
    fn searching_hides_the_favourites_and_empty_groups() {
        let groups = vec![group("Color Negative", &["gold", "portra"]), group("Black & White", &["hp5"])];
        let shown = sections(matching("portra", &groups), entries(&["portra"]), "portra");
        assert_eq!(shown, vec![group("Color Negative", &["portra"])]);
        // Spaces alone aren't a search.
        assert_eq!(sections(matching("  ", &groups), entries(&["portra"]), "  ")[0].title, FAVOURITES);
        assert!(sections(matching("nothing", &groups), vec![], "nothing").is_empty());
        assert_eq!(no_match(" nothing ", false), "No films match \u{201c}nothing\u{201d}");
        assert_eq!(matches(0), "");
        assert_eq!(matches(1), "1 match · return tries the first");
        assert_eq!(matches(3), "3 matches · return tries the first");
    }

    #[test]
    fn stepping_goes_through_the_favourites_and_on_into_the_groups() {
        let p = places(&row());
        // From the favourites' copy of Portra: on to Trix, the next favourite.
        assert_eq!(step("portra", true, 1, &p), Some(Place { stock: "trix".into(), favourite: true }));
        // Trix is the last favourite; the first group's Gold comes next.
        assert_eq!(step("trix", true, 1, &p), Some(Place { stock: "gold".into(), favourite: false }));
        // Back from Gold to the favourites.
        assert_eq!(step("gold", false, -1, &p), Some(Place { stock: "trix".into(), favourite: true }));
        // From Portra's own copy in its group: Gold and Ektar either side.
        assert_eq!(step("portra", false, 1, &p).unwrap().stock, "ektar");
        assert_eq!(step("portra", false, -1, &p).unwrap().stock, "gold");
        // Held at either end.
        assert_eq!(step("portra", true, -1, &p), Some(Place { stock: "portra".into(), favourite: true }));
        assert_eq!(step("trix", false, 1, &p), Some(Place { stock: "trix".into(), favourite: false }));
        // Not in the row: the first.
        assert_eq!(step("velvia", false, 1, &p).unwrap().stock, "portra");
        assert_eq!(step("x", false, 1, &[]), None);
    }

    #[test]
    fn a_favourite_next_to_its_own_copy_is_passed_over() {
        // The last favourite is the first group's first stock.
        let row = sections(vec![group("Color Negative", &["gold", "portra"])], entries(&["gold"]), "");
        let p = places(&row);
        assert_eq!(step("gold", true, 1, &p).unwrap().stock, "portra");
        assert_eq!(step("portra", false, -1, &p), Some(Place { stock: "gold".into(), favourite: false }));
        assert_eq!(step("gold", false, -1, &p), Some(Place { stock: "gold".into(), favourite: false }), "held: the copy before it is the same stock");
    }

    #[test]
    fn stepping_while_searching_goes_through_the_matches() {
        let groups = vec![group("Color Negative", &["gold", "portra160", "portra400"]), group("Motion Picture", &["vision", "portra800"])];
        let row = sections(matching("portra", &groups), entries(&["gold"]), "portra");
        let p = places(&row);
        assert_eq!(p.iter().map(|p| p.stock.as_str()).collect::<Vec<_>>(), ["portra160", "portra400", "portra800"]);
        assert_eq!(step("portra400", true, 1, &p).unwrap().stock, "portra800");
        // The stock tried before the search isn't a match: the first.
        assert_eq!(step("gold", true, 1, &p).unwrap().stock, "portra160");
    }

    #[test]
    fn the_row_is_laid_out_as_on_the_mac() {
        let row = row();
        assert_eq!(width(&row[0]), 18.0 + 9.0 + 2.0 * 212.0 + 8.0);
        let s = starts(&row);
        assert_eq!(s[0], INSET);
        assert_eq!(s[1], INSET + width(&row[0]) + GROUP_GAP);
        assert_eq!(content_width(&row), 2.0 * INSET + row.iter().map(width).sum::<f64>() + 2.0 * GROUP_GAP);
        assert_eq!(content_width(&[]), 2.0 * INSET);
        assert_eq!(tile_x("ektar", 1, &row), Some(s[1] + 27.0 + 2.0 * 220.0));
        assert_eq!(tile_x("ektar", 0, &row), None);
        assert_eq!(section_of("portra", &row, true), Some(0));
        assert_eq!(section_of("portra", &row, false), Some(1));
        assert_eq!(section_of("gold", &row, true), Some(1));
        assert_eq!(current(0.0, &s), 0);
        assert_eq!(current(s[1] - 40.0, &s), 1);
        assert_eq!(current(s[1] - 41.0, &s), 0);
    }

    #[test]
    fn opening_centres_the_current_tile_and_steps_only_reveal_it() {
        let row = row();
        let content = content_width(&row);
        let x = tile_x("ektar", 1, &row).unwrap();
        assert_eq!(reveal(0.0, "ektar", true, true, &row, 600.0), clamp(x - (600.0 - TILE) / 2.0, 600.0, content));
        // In view already: a step leaves the row alone.
        let offset = x - 100.0;
        assert_eq!(reveal(offset, "ektar", true, false, &row, 600.0), offset);
        // Out of view: centred.
        assert_eq!(reveal(0.0, "trix", false, false, &row, 600.0), centring(tile_x("trix", 2, &row).unwrap(), 600.0, content));
        // Held within the row.
        assert_eq!(centring(5.0, 600.0, content), 0.0);
        assert_eq!(clamp(1.0e6, 600.0, content), content - 600.0);
        assert_eq!(clamp(50.0, 600.0, 400.0), 0.0);
        assert_eq!(jump(starts(&row)[1], 600.0, content), starts(&row)[1] - 30.0);
    }

    #[test]
    fn the_wheel_scrolls_sideways() {
        assert_eq!(wheel(200.0, 120, false, 600.0, 2000.0), 200.0 - WHEEL_NOTCH);
        assert_eq!(wheel(200.0, -240, false, 600.0, 2000.0), 200.0 + 2.0 * WHEEL_NOTCH);
        assert_eq!(wheel(200.0, 120, true, 600.0, 2000.0), 200.0 + WHEEL_NOTCH);
        assert_eq!(wheel(10.0, 120, false, 600.0, 2000.0), 0.0);
        assert_eq!(wheel(1390.0, -120, false, 600.0, 2000.0), 1400.0);
    }

    #[test]
    fn clicks_find_the_tile_or_its_star() {
        let row = row();
        let left = tile_left(starts(&row)[1], 1);
        assert_eq!(hit(&row, left + 10.0, TOP + 50.0), Some(Hit::Tile(1, 1)));
        assert_eq!(hit(&row, left + TILE - 10.0, TOP + 10.0), Some(Hit::Star(1, 1)));
        assert_eq!(hit(&row, left + 10.0, TOP + PICTURE + 10.0), Some(Hit::Tile(1, 1)), "the name is the tile's too");
        assert_eq!(hit(&row, left - 4.0, TOP + 50.0), None, "the gap");
        assert_eq!(hit(&row, starts(&row)[1] + 5.0, TOP + 50.0), None, "the group's title");
        assert_eq!(hit(&row, left + 10.0, 2.0), None);
        assert_eq!(hit(&row, left + 10.0, ROW - 1.0), None);
    }

    #[test]
    fn the_foot_says_how_to_try_then_how_to_keep_or_go_back() {
        assert_eq!(hint(None), "click to try · ← → to step");
        assert_eq!(hint(Some("Kodak Gold 200")), "return keeps · esc goes back to Kodak Gold 200");
        assert_eq!(done_help(true), "Keep this paper (Return)");
        assert_eq!(keep_paper_words(true), "Kept for every film");
        assert_eq!(star_help(true), "Remove from favourites");
        let visit = Visit::new(false, (), "gold");
        assert!(!visit.shows_before("gold", "gold"));
        assert!(visit.shows_before("gold", "portra"));
        assert!(!visit.shows_before("portra", "portra"));
        assert_eq!(visit.action(), "Change Film");
        assert_eq!(Visit::new(true, (), "x").action(), "Change Paper");
    }

    /// A visit's tries, as the model makes them: tries replace the edit
    /// with no undo step; keeping is one step from the edit before the
    /// visit, going back restores it with none.
    struct Session {
        state: &'static str,
        undo: Vec<(&'static str, &'static str)>,
        visit: Option<Visit<&'static str>>,
    }

    impl Session {
        fn open(&mut self) {
            self.visit = Some(Visit::new(false, self.state, self.state));
        }
        fn try_stock(&mut self, stock: &'static str) {
            assert!(self.visit.is_some());
            self.state = stock;
        }
        fn end(&mut self, keep: bool) {
            let visit = self.visit.take().unwrap();
            match ending(keep, visit.before, &self.state, visit.action()) {
                Ending::Nothing => {}
                Ending::Step(before, action) => self.undo.push((before, action)),
                Ending::Restore(before) => self.state = before,
            }
        }
        fn undo(&mut self) {
            if let Some((before, _)) = self.undo.pop() {
                self.state = before;
            }
        }
    }

    #[test]
    fn a_visit_is_one_undo_step_and_esc_goes_back() {
        let mut s = Session { state: "gold", undo: vec![], visit: None };
        s.open();
        for stock in ["portra", "ektar", "trix", "hp5"] {
            s.try_stock(stock);
        }
        s.end(true);
        assert_eq!((s.state, s.undo.clone()), ("hp5", vec![("gold", "Change Film")]), "four tries, one step");
        s.undo();
        assert_eq!(s.state, "gold", "undo undoes the whole visit");

        s.open();
        s.try_stock("portra");
        s.try_stock("ektar");
        s.end(false);
        assert_eq!((s.state, s.undo.len()), ("gold", 0), "Esc puts the stock back, without a step");

        s.open();
        s.try_stock("portra");
        s.try_stock("gold");
        s.end(true);
        assert!(s.undo.is_empty(), "back where it opened: no step");
        assert_eq!(ending(false, 1, &1, "x"), Ending::Nothing);
    }

    #[test]
    fn search_keys_try_the_first_match_or_keep_and_clear_or_go_back() {
        assert_eq!(submit("portra"), Submit::TryFirst);
        assert_eq!(submit(" "), Submit::Keep);
        assert_eq!(cancel("p"), Cancel::Clear);
        assert_eq!(cancel(""), Cancel::GoBack);
    }

    #[test]
    fn the_tray_takes_esc_return_and_the_arrows_unless_typing_or_a_command() {
        assert_eq!(tray_key(Key::Escape, false, false), TrayKey::GoBack);
        assert_eq!(tray_key(Key::Return, false, false), TrayKey::Keep);
        assert_eq!(tray_key(Key::Left, false, false), TrayKey::Step(-1));
        assert_eq!(tray_key(Key::Right, false, false), TrayKey::Step(1));
        assert_eq!(tray_key(Key::Other, false, false), TrayKey::Pass);
        assert_eq!(tray_key(Key::Escape, false, true), TrayKey::Pass, "typing keeps its keys");
        assert_eq!(tray_key(Key::Left, true, false), TrayKey::Pass, "Ctrl+← is a command");
    }

    #[test]
    fn esc_goes_to_the_first_that_applies() {
        let all = EscapeContext { alert: true, typing: true, tray: true, cropping: true, grid: true, image_only: true, photograph: true };
        assert_eq!(escape(all), Escape::Alert);
        assert_eq!(escape(EscapeContext { alert: false, ..all }), Escape::Typing);
        assert_eq!(escape(EscapeContext { alert: false, typing: false, ..all }), Escape::Tray);
        assert_eq!(escape(EscapeContext { tray: true, image_only: true, photograph: true, ..Default::default() }), Escape::Tray, "the tray before Image Only");
        assert_eq!(escape(EscapeContext { cropping: true, image_only: true, ..Default::default() }), Escape::Crop);
        assert_eq!(escape(EscapeContext { grid: true, photograph: true, ..Default::default() }), Escape::Grid);
        assert_eq!(escape(EscapeContext { image_only: true, photograph: true, ..Default::default() }), Escape::ImageOnly);
        assert_eq!(escape(EscapeContext { photograph: true, ..Default::default() }), Escape::ShowGrid);
        assert_eq!(escape(EscapeContext::default()), Escape::Nothing);
    }

    #[test]
    fn the_stand_in_has_the_photographs_long_side() {
        assert_eq!(stand_in_size((320, 213), (6016.0, 4016.0)), (6016.0, 213.0 * 6016.0 / 320.0));
        assert_eq!(stand_in_size((213, 320), (4016.0, 6016.0)), (213.0 * 6016.0 / 320.0, 6016.0));
        assert_eq!(stand_in_size((320, 160), (0.0, 0.0)), (2048.0, 1024.0));
    }
}
