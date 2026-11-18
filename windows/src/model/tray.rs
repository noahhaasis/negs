//! The film and paper tray (IMPLEMENTATION.md §4.5, §5.1; `EditorModel`'s
//! tray in `EditorModel.swift`): a click on a tile tries its stock and the
//! tray stays open; Return, Done, a click outside, a command or any other
//! edit keep it, as one undo step for the whole visit; Esc goes back to the
//! stock from before. The rules are in `tray_rules.rs`; the window in
//! `ui/tray.rs`.

use super::*;
use crate::previews::{Trial, TrayVisit};
use crate::tray_rules::{self as rules, Cancel, Ending, Entry, Section, Submit};

impl Model {
    /// The tray, while it's open.
    pub fn tray(&self) -> Option<&TrayVisit> {
        self.previews.tray.as_ref()
    }

    /// The film's or the paper's tray can open: tiles are on (Settings),
    /// a photograph is open and not being cropped, and for papers it's printed.
    pub fn can_open_tray(&self, paper: bool) -> bool {
        self.prefs.previews && self.source.is_some() && self.has_catalog() && !self.cropping() && !(paper && json::boolean(self.value("io.scan_film")))
    }

    /// The open tray, film (false) or paper (true), while its tiles are on
    /// screen and may be made.
    pub fn tray_shown(&self) -> Option<bool> {
        self.tray().map(|t| t.paper).filter(|&paper| self.can_open_tray(paper))
    }

    /// Opens the film or paper tray on the current stock, keeping whatever
    /// the other tray was trying.
    pub fn open_tray(&mut self, paper: bool) {
        if !self.can_open_tray(paper) {
            return;
        }
        if let Some(tray) = &self.previews.tray {
            if tray.paper == paper {
                return;
            }
            self.end_tray(true);
        }
        let id = self.tray_current(paper, false);
        self.previews.tray = Some(TrayVisit::new(paper, self.state.clone(), id));
        self.mark(dirty::TRAY | dirty::LAYOUT | dirty::PREVIEWS | dirty::CANVAS | dirty::INSPECTOR);
    }

    /// The tile the tray rings: the paper, Your negative while the
    /// photograph is read as one, else the film (or the film it's read as).
    pub fn tray_current(&self, paper: bool, read_as: bool) -> String {
        if paper {
            self.state.paper.clone()
        } else if self.is_negative() && !read_as {
            crate::negative::STOCK_ID.into()
        } else {
            self.state.film.clone()
        }
    }

    /// The negative's "read as" name: the colour negatives it can be read
    /// as, in the film tray; a second click keeps the one tried.
    pub fn toggle_read_as_tray(&mut self) {
        if self.tray().is_some_and(|t| !t.paper && t.read_as) {
            return self.end_tray(true);
        }
        if !self.is_negative() || !self.can_open_tray(false) {
            return;
        }
        self.end_tray(true);
        let mut visit = TrayVisit::new(false, self.state.clone(), self.state.film.clone());
        visit.read_as = true;
        self.previews.tray = Some(visit);
        self.mark(dirty::TRAY | dirty::LAYOUT | dirty::PREVIEWS | dirty::CANVAS | dirty::INSPECTOR);
    }

    /// A click on a name: opens its tray, or closes it keeping the stock.
    pub fn toggle_tray(&mut self, paper: bool) {
        if self.previews.open(paper) && !self.tray().is_some_and(|t| t.read_as) {
            self.end_tray(true);
        } else {
            self.open_tray(paper);
        }
    }

    /// Tries a stock on the photograph; the tray stays open. Tries aren't
    /// undo steps: keeping one is (`end_tray`). The stock's tile, if it
    /// shows exactly the edit tried, stands in on the canvas until that
    /// edit's develop is in.
    pub fn try_stock(&mut self, id: &str) {
        let Some((paper, read_as)) = self.tray().map(|t| (t.paper, t.read_as)) else { return };
        let Some(source) = self.source.clone() else { return };
        if id == crate::negative::STOCK_ID && !paper && !read_as {
            return self.try_negative();
        }
        // Only the tray's own kind: a film is never printed on as a paper.
        let Some(stock) = self.stocks.iter().find(|s| s.paper == paper && s.id == id) else { return };
        if read_as && !crate::negative::readable(stock) {
            return;
        }
        let next = self.choosing_stock(&self.state, id, paper, read_as);
        if next == self.state {
            return;
        }
        // The tile shows this edit if it was made from the request it would
        // be made from before the change.
        let generation = self.source_generation;
        let key = previews::Key::new(generation, &self.tile_command(&self.render_state(), stock, &source, generation));
        self.replace_state(next);
        self.mark(dirty::TRAY);
        if self.preview_current {
            return;
        }
        let target = Target::Stock { id: id.to_owned(), paper };
        let picture = self.previews.thumb(&target).filter(|t| t.shows(&key)).and_then(|t| t.image.as_ref()).and_then(|b| b.source.as_ref()).map(|s| {
            let mut picture = crate::picture::Picture::new(s.rgba.clone(), s.width, s.height);
            picture.wide = s.wide;
            picture
        });
        if let Some(picture) = picture {
            self.previews.trial = Some(Trial { picture, revision: self.revision, generation });
            self.mark(dirty::CANVAS);
        }
    }

    /// The stand-in on the canvas: the tile of the stock just tried, until
    /// a frame of that edit is shown, unless the original is.
    pub fn shown_trial(&self) -> Option<&Trial> {
        let trial = self.previews.trial.as_ref()?;
        let current = (trial.revision, trial.generation) == (self.revision, self.source_generation);
        let developed = self.shown_frame.is_some_and(|f| (f.revision, f.generation) == (self.revision, self.source_generation));
        (current && !developed && self.original.is_none() && !self.cropping() && !self.render_failed).then_some(trial)
    }

    /// ← →: the previous or next stock, in the tray's order, through the
    /// favourites and on into the groups; among the matches while searching.
    pub fn step_tray(&mut self, offset: i32) {
        let Some(tray) = self.tray() else { return };
        let current = self.tray_current(tray.paper, tray.read_as);
        let places = rules::places(&self.tray_sections());
        let Some(place) = rules::step(&current, tray.in_favourites, offset, &places) else { return };
        if let Some(tray) = &mut self.previews.tray {
            tray.in_favourites = place.favourite;
        }
        self.try_stock(&place.stock);
        // The row follows the stock stepped to, even when it was already tried.
        self.mark(dirty::TRAY);
    }

    /// A tile clicked: its stock, from its copy among the favourites or in its group.
    pub fn choose_tile(&mut self, id: &str, favourites: bool) {
        if let Some(tray) = &mut self.previews.tray {
            tray.in_favourites = favourites;
        }
        self.try_stock(id);
    }

    /// Return, Done, a click outside, a command or another edit keep the
    /// stock being tried, as one undo step from the stock before the tray
    /// opened; Esc puts that one back (§5.1). Either closes the tray.
    pub fn end_tray(&mut self, keep: bool) {
        let Some(tray) = self.previews.tray.take() else { return };
        let before = self.with_preferences(tray.before.clone());
        match rules::ending(keep, before, &self.state, tray.action()) {
            Ending::Nothing => {}
            Ending::Step(before, action) => {
                self.register_undo(before, action);
                // The kept stock reaches the sidecar now, once.
                self.library_edited();
            }
            Ending::Restore(before) => self.replace_state(before),
        }
        self.negative_tray_ended();
        self.mark(dirty::TRAY | dirty::LAYOUT | dirty::PREVIEWS | dirty::CANVAS | dirty::INSPECTOR | dirty::CHROME);
    }

    /// The tray's groups as shown (`StockTray.sections`): the favourites
    /// first, then each group; while searching only the matches.
    pub fn tray_sections(&self) -> Vec<Section> {
        let Some(tray) = self.tray() else { return vec![] };
        let (paper, read_as) = (tray.paper, tray.read_as);
        let (mut films, papers) = (self.listed_films(), self.papers());
        // Reading a negative: the colour negatives only.
        if read_as {
            films.retain(|s| crate::negative::readable(s));
        }
        let groups = if paper { state::paper_groups(&papers) } else { state::matching(&tray.query, state::film_groups(&films, &papers)) };
        let entry = |s: &Stock| Entry::new(s.id.clone(), s.name.clone());
        let matched = groups.into_iter().map(|g| Section::group(g.title, g.stocks.into_iter().map(entry).collect())).collect();
        // Films listed only (Settings › Fotufilm films).
        let favourites = self.favorite_stocks(paper).into_iter().filter(|s| paper || films.iter().any(|f| f.id == s.id)).map(entry).collect();
        rules::with_scan(rules::sections(matched, favourites, &tray.query), paper, read_as, &tray.query, crate::negative::NAME)
    }

    /// The name of the stock the tray opened on, once another is tried.
    pub fn tray_before_name(&self) -> Option<String> {
        let tray = self.tray()?;
        let current = self.tray_current(tray.paper, tray.read_as);
        if tray.before_id == crate::negative::STOCK_ID {
            return tray.tried(&current).then(|| crate::negative::NAME.into());
        }
        tray.tried(&current).then(|| self.stocks.iter().find(|s| s.paper == tray.paper && s.id == tray.before_id).map_or_else(|| tray.before_id.clone(), |s| s.name.clone()))
    }

    /// Typing in Search filters the film tray at once, and never edits the photograph.
    pub fn set_tray_query(&mut self, query: &str) {
        let Some(tray) = self.previews.tray.as_mut().filter(|t| !t.paper && t.query != query) else { return };
        tray.query = query.to_owned();
        // The thumbnails asked for follow the matches.
        self.mark(dirty::TRAY | dirty::PREVIEWS);
    }

    /// Return in Search tries the first match; with nothing typed it keeps.
    pub fn tray_submit(&mut self) {
        let Some(tray) = self.tray() else { return };
        match rules::submit(&tray.query) {
            Submit::Keep => self.end_tray(true),
            Submit::TryFirst => {
                let first = self.tray_sections().into_iter().find(|s| !s.favourites).and_then(|s| s.stocks.into_iter().next());
                if let Some(first) = first {
                    self.choose_tile(&first.id, false);
                }
            }
        }
    }

    /// Esc in Search clears it; in an empty Search it goes back, as anywhere in the tray.
    pub fn tray_cancel_search(&mut self) {
        let Some(tray) = self.tray() else { return };
        match rules::cancel(&tray.query) {
            Cancel::Clear => self.set_tray_query(""),
            Cancel::GoBack => self.end_tray(false),
        }
    }
}
