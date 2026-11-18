//! Your negative (ticket 33; `EditorModel`'s negative half): reading the
//! photograph as a scan of a colour negative, finding its film base,
//! balancing and metering its print, the pickers and Crop to picture. The
//! words and pure rules are in `crate::negative`.
//!
//! Each request runs on a thread of its own that asks the helper in turn
//! (the picture's survey, the base, the meter) and lands on the UI thread
//! only if it's still the newest and the photograph is still read the way
//! it asked about; a newer request supersedes it.

use super::*;
use crate::negative::{self as neg, BaseStatus, Pick};
use std::sync::atomic::{AtomicU64, Ordering};

/// How long a detection or balance that follows an edit waits for the next.
const SETTLE: Duration = Duration::from_millis(200);

/// What the negative's requests remember about this photograph.
#[derive(Default)]
pub struct Memory {
    /// A request about the negative is running: what Prep says meanwhile.
    pub working: Option<String>,
    /// The canvas takes the next click as a point to read.
    pub pick: Option<Pick>,
    /// The newest request; an older one's answer is dropped.
    request: Arc<AtomicU64>,
    /// The newest request finds the base (and balances for it).
    detecting: bool,
    /// What the rebate finder read, for each way of reading lately.
    surveys: Vec<Survey>,
    /// Each base found or picked on this photograph, and how, newest last.
    bases: Vec<(u64, Value, BaseStatus)>,
    /// Where the base was picked, kept for a re-detect after a change of
    /// film or scan light.
    point: Option<(u64, [f64; 2])>,
    /// Answers folded into the edit that asked: undo or redo putting that
    /// edit back puts the answer back too.
    folded: Vec<(u64, EditState, Vec<(String, Value)>)>,
    /// Your negative was tried in the tray: crop to the picture if it's kept.
    crop_pending: Option<u64>,
    /// The next `replace_state` is Undo or Redo putting an edit back.
    pub(super) undoing: bool,
    /// Prep's "Backlight…" was clicked: the Backlight row shows, until
    /// another photograph opens.
    pub backlight_shown: bool,
}

/// What `negative_base` read along the film's edge for one reading.
#[derive(Clone, Debug)]
pub struct Survey {
    generation: u64,
    key: Option<String>,
    picture: Value,
    looks_negative: bool,
    base: Value,
    found: bool,
    edge: Option<String>,
}

impl Survey {
    fn of(generation: u64, key: Option<String>, reply: &Value) -> Self {
        let status = BaseStatus::of(reply);
        Self {
            generation,
            key,
            picture: reply["picture"].clone(),
            looks_negative: json::boolean(&reply["looks_negative"]),
            base: reply["base"].clone(),
            found: status.found,
            edge: status.edge,
        }
    }
}

/// A request's helper calls, off the UI thread.
struct Asker {
    engine: Arc<EngineClient>,
    /// The command's common fields: the photograph, film, paper and params asked about.
    template: Value,
    generation: u64,
    key: Option<String>,
    survey: Option<Survey>,
    /// The survey was read by this request: the model keeps it.
    fresh: bool,
}

impl Asker {
    fn ask(&self, name: &str, changes: &[(String, Value)], extra: &[(&str, Value)]) -> Result<Value> {
        let mut command = self.template.clone();
        command["command"] = json!(name);
        for (path, value) in changes {
            json::set(&mut command["params"], path, value.clone());
        }
        for (key, value) in extra {
            command[*key] = value.clone();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.engine.request(command, engine::timeout_for(name), move |result| {
            let _ = tx.send(result);
        });
        Ok(rx.recv().map_err(|_| anyhow!("The helper stopped"))??.1)
    }

    /// The picture and base a detection finds on this reading, read once.
    fn survey(&mut self) -> Result<&Survey> {
        if self.survey.is_none() {
            let reply = self.ask("negative_base", &[], &[])?;
            self.note_survey(&reply);
        }
        Ok(self.survey.as_ref().expect("surveyed"))
    }

    /// A detection without a point is this reading's survey too.
    fn note_survey(&mut self, reply: &Value) {
        self.survey = Some(Survey::of(self.generation, self.key.clone(), reply));
        self.fresh = true;
    }

    /// The picture Auto and the meter read in; None for the frame less a border.
    fn bounds(&mut self) -> Option<Value> {
        self.survey().ok().and_then(|s| neg::bounds(&s.picture))
    }

    /// The print exposure the params with `changes` meter to; None if it
    /// can't be metered (the exposure then stays).
    fn meter(&mut self, changes: &[(String, Value)]) -> Option<Value> {
        let bounds = self.bounds();
        let extra: Vec<(&str, Value)> = bounds.into_iter().map(|b| ("bounds", b)).collect();
        self.ask("negative_meter", changes, &extra).ok().map(|r| r["print_exposure"].clone()).filter(|v| !v.is_null())
    }
}

fn answer_key(state: &EditState) -> Option<String> {
    neg::key(state).map(|k| format!("{k}|{}", state.paper))
}

fn filtration(state: &EditState) -> (Value, Value) {
    (state.value("enlarger.m_filter_shift").clone(), state.value("enlarger.y_filter_shift").clone())
}

/// "Magenta 4.4, yellow −1.8, print exposure 0.87".
fn balance_sentence(changes: &[(String, Value)], decimal: char) -> String {
    let get = |path: &str| changes.iter().find(|(p, _)| p == path).map(|(_, v)| json::number(v));
    let f = |v: f64, places: usize| format!("{v:.places$}").replace('-', "−").replace('.', &decimal.to_string());
    let exposure = get("enlarger.print_exposure").map_or_else(String::new, |e| format!(", print exposure {}", f(e, 2)));
    format!("Magenta {}, yellow {}{exposure}", f(get("enlarger.m_filter_shift").unwrap_or(0.0), 1), f(get("enlarger.y_filter_shift").unwrap_or(0.0), 1))
}

impl Model {
    pub fn is_negative(&self) -> bool {
        neg::is_negative(&self.state)
    }

    /// The film a negative is read as: the current one if it's a colour
    /// negative, else Portra 400 (decision 2: say so, no averaged profile).
    fn negative_film(&self, state: &EditState) -> Option<String> {
        let films = || self.stocks.iter().filter(|s| !s.paper);
        if let Some(film) = films().find(|s| s.id == state.film && neg::readable(s)) {
            return Some(film.id.clone());
        }
        if films().any(|s| s.id == neg::DEFAULT_FILM) {
            return Some(neg::DEFAULT_FILM.into());
        }
        films().find(|s| neg::readable(s)).map(|s| s.id.clone())
    }

    /// `state` read as a negative.
    pub fn negative_state(&self, from: &EditState) -> EditState {
        let mut next = from.clone();
        json::set(&mut next.params, neg::ROUTE_PATH, json!(neg::ROUTE));
        if let Some(film) = self.negative_film(from).filter(|f| *f != from.film) {
            // Entering, from a slide say, the film's own paper comes too
            // (unless the paper is locked); changing the read-as film later keeps it.
            next = state::choosing(&next, &film, false, true, &self.stocks);
            state::pair_film(&mut next, &self.stocks, !self.prefs.keep_paper);
        }
        // A negative is read and printed, never scanned as film.
        json::set(&mut next.params, "io.scan_film", json!(false));
        // The camera's preview is of the orange negative: nothing to match.
        next.brightness_match = None;
        next
    }

    /// `id` chosen in the tray or a menu: a film leaves a negative (unless
    /// it's the film the negative is read as), a paper keeps it.
    pub fn choosing_stock(&self, state: &EditState, id: &str, paper: bool, read_as: bool) -> EditState {
        if read_as && !paper {
            let mut next = state::choosing(state, id, false, true, &self.stocks);
            json::set(&mut next.params, "io.scan_film", json!(false));
            return next;
        }
        let from = if !paper && neg::is_negative(state) { neg::leaving(state, &self.defaults) } else { state.clone() };
        state::choosing(&from, id, paper, self.prefs.keep_paper, &self.stocks)
    }

    /// A stock tile's helper request: as choosing it would develop, and on
    /// a negative its base found for another film, or Auto for another paper.
    pub(super) fn tile_command(&self, state: &EditState, stock: &Stock, source: &Path, generation: u64) -> Value {
        let read_as = self.tray().is_some_and(|t| t.read_as);
        let negative = neg::is_negative(state);
        let from = if negative && !stock.paper && !read_as { neg::leaving(state, &self.defaults) } else { state.clone() };
        let keep_paper = self.prefs.keep_paper || (read_as && !stock.paper);
        let mut command = previews::stock_command(&from, stock, &self.stocks, keep_paper, source, generation);
        if negative && read_as && !stock.paper && stock.id != state.film {
            command["params"]["io"]["scan_film"] = json!(false);
            let point = self.negative.point.filter(|(g, _)| *g == generation).map(|(_, p)| p);
            command["read_negative"] = point.map_or_else(|| json!({}), |p| json!({"point": p}));
        }
        if negative && stock.paper && stock.id != state.paper {
            let bounds = self.negative_survey().and_then(|s| neg::bounds(&s.picture));
            command["auto_negative"] = bounds.map_or_else(|| json!({}), |b| json!({"bounds": b}));
        }
        command
    }

    /// The label's "Your negative" outside the tray: reads the photograph
    /// as a negative, finds its base and balances the print, one undo step.
    pub fn choose_negative(&mut self) {
        if self.is_negative() || self.source.is_none() || self.cropping() {
            return;
        }
        let next = self.negative_state(&self.state);
        self.apply(next, "Read as Negative");
    }

    /// The film a negative is read as, from the plain menu.
    pub fn choose_negative_film(&mut self, id: &str) {
        let next = self.choosing_stock(&self.state, id, false, true);
        self.apply(next, "Change Negative Film");
    }

    /// The tray's "Your negative" tried: read as a negative; the base is
    /// found as part of the try.
    pub(super) fn try_negative(&mut self) {
        if !self.is_negative() {
            let next = self.negative_state(&self.state);
            self.replace_state(next);
            self.mark(dirty::TRAY);
        }
    }

    /// The film tray kept Your negative: crop to the picture it found.
    pub(super) fn negative_tray_ended(&mut self) {
        if self.negative.crop_pending.take() == Some(self.source_generation) && self.is_negative() {
            self.crop_to_found_picture();
        }
    }

    /// What the rebate finder read on this photograph as it's read now.
    fn negative_survey(&self) -> Option<&Survey> {
        let key = neg::key(&self.state)?;
        self.negative.surveys.iter().rev().find(|s| s.generation == self.source_generation && s.key.as_deref() == Some(key.as_str()))
    }

    fn keep_survey(&mut self, survey: Survey) {
        self.negative.surveys.retain(|s| s.generation == survey.generation && s.key != survey.key);
        self.negative.surveys.push(survey);
        if self.negative.surveys.len() > 8 {
            self.negative.surveys.remove(0);
        }
    }

    fn record_base(&mut self, base: Value, status: BaseStatus, generation: u64) {
        self.negative.bases.retain(|(g, b, _)| *g == generation && !neg::same_base(b, &base));
        self.negative.bases.push((generation, base, status));
        if self.negative.bases.len() > 32 {
            self.negative.bases.remove(0);
        }
    }

    /// Where the film base now shown came from, for Prep: what the
    /// detection that made it said, else how it compares with what a
    /// detection finds. None while that isn't known.
    pub fn negative_base(&self) -> Option<BaseStatus> {
        if !self.is_negative() {
            return None;
        }
        let base = self.state.value("film_render.base");
        if let Some((_, _, status)) = self.negative.bases.iter().rev().find(|(g, b, _)| *g == self.source_generation && neg::same_base(b, base)) {
            return Some(status.clone());
        }
        if neg::same_base(base, json::get(&self.defaults, "film_render.base")) {
            return None;
        }
        let survey = self.negative_survey().filter(|s| !s.base.is_null())?;
        if neg::same_base(&survey.base, base) {
            return Some(BaseStatus { found: survey.found, picked: false, edge: survey.edge.clone(), by_hand: false });
        }
        Some(BaseStatus { found: false, picked: false, edge: None, by_hand: true })
    }

    /// The scan doesn't look like a colour negative, in Prep's words.
    pub fn negative_warning(&self) -> Option<&'static str> {
        let survey = self.negative_survey()?;
        (self.is_negative() && !survey.looks_negative).then(|| neg::not_negative(self.is_raw))
    }

    /// Prep's Film base line.
    pub fn negative_sentence(&self) -> String {
        if let Some(working) = &self.negative.working {
            return working.clone();
        }
        match self.negative_base() {
            Some(base) => base.sentence(self.state.orientation()),
            None => format!("Reading the negative as {}.", self.film_name()),
        }
    }

    pub fn negative_busy(&self) -> bool {
        self.negative.working.is_some()
    }

    pub fn negative_pick(&self) -> Option<Pick> {
        self.negative.pick
    }

    /// Prep's pickers: the canvas takes the next click as a point to read.
    pub fn begin_negative_pick(&mut self, pick: Pick) {
        if !self.is_negative() || self.negative_busy() {
            return;
        }
        self.end_tray(true);
        self.negative.pick = Some(pick);
        self.mark(dirty::CANVAS | dirty::INSPECTOR | dirty::STATUS);
    }

    /// Esc while picking.
    pub fn cancel_negative_pick(&mut self) -> bool {
        let picking = self.negative.pick.take().is_some();
        if picking {
            self.mark(dirty::CANVAS | dirty::INSPECTOR | dirty::STATUS);
        }
        picking
    }

    /// A click on the canvas while picking, at `shown` (fractions of the
    /// photograph as shown): back through the flip, turns and crop to the
    /// whole photograph. The crop's straightening isn't undone: it moves a
    /// point a few pixels at most, inside the area the reading averages.
    pub fn picked_shown_point(&mut self, shown: [f64; 2]) {
        let p = self.state.orientation().unshown(crate::crop::Point { x: shown[0], y: shown[1] });
        let inside = (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y);
        let r = self.state.crop.rect;
        let point = if inside { [r.x + p.x * r.width, r.y + p.y * r.height] } else { [-1.0, -1.0] };
        self.picked_negative_point(point);
    }

    /// A click on the canvas while picking: `point` is a fraction of the
    /// whole, uncropped photograph, from its top left.
    pub fn picked_negative_point(&mut self, point: [f64; 2]) {
        let Some(pick) = self.negative.pick.take() else { return };
        self.mark(dirty::CANVAS | dirty::INSPECTOR | dirty::STATUS);
        if !(0.0..=1.0).contains(&point[0]) || !(0.0..=1.0).contains(&point[1]) {
            return;
        }
        match pick {
            Pick::Base => self.detect_negative_base(Some(point), true, false),
            Pick::Neutral => self.negative_print("negative_neutral", Some(point), "Balancing on the grey…", "Pick Neutral"),
        }
    }

    fn begin_negative(&mut self, working: &str, detecting: bool) -> u64 {
        let request = self.negative.request.fetch_add(1, Ordering::Relaxed) + 1;
        self.negative.working = Some(working.into());
        self.negative.detecting = detecting;
        self.mark(dirty::INSPECTOR);
        request
    }

    fn end_negative(&mut self, request: u64) {
        if self.negative.request.load(Ordering::Relaxed) == request {
            self.negative.working = None;
            self.negative.detecting = false;
            self.mark(dirty::INSPECTOR);
        }
    }

    /// Runs `work` off the UI thread with the helper, then `done` with its
    /// answer if it's still the newest request and the photograph is still
    /// read as it asked. `settle`: wait a moment first, so trying film after
    /// film reads only the last.
    fn run_negative<T: Send + 'static>(
        &mut self,
        working: &str,
        detecting: bool,
        settle: bool,
        work: impl FnOnce(&mut Asker) -> Result<T> + Send + 'static,
        done: impl FnOnce(&mut Model, T) + Send + 'static,
    ) {
        let (Some(engine), Some(source)) = (self.engine.clone(), self.source.clone()) else { return };
        let snapshot = self.render_state();
        let generation = self.source_generation;
        let mut asker = Asker {
            engine,
            template: json!({"input": source, "source_generation": generation, "film": snapshot.film, "paper": snapshot.paper, "params": snapshot.params}),
            generation,
            key: neg::key(&snapshot),
            survey: self.negative_survey().cloned(),
            fresh: false,
        };
        let request = self.begin_negative(working, detecting);
        let newest = self.negative.request.clone();
        let asked = answer_key(&self.state);
        #[cfg(test)]
        let background = ui::testing::background();
        std::thread::Builder::new()
            .name("negative".into())
            .spawn(move || {
                #[cfg(test)]
                let _background = background;
                if settle {
                    std::thread::sleep(SETTLE);
                }
                let result = if newest.load(Ordering::Relaxed) == request { Some(work(&mut asker)) } else { None };
                let survey = asker.fresh.then_some(asker.survey).flatten();
                ui::post(move |app: &mut App| {
                    let model = &mut app.model;
                    if let Some(survey) = survey.filter(|s| s.generation == model.source_generation) {
                        model.keep_survey(survey);
                    }
                    let current = model.negative.request.load(Ordering::Relaxed) == request
                        && generation == model.source_generation
                        && answer_key(&model.state) == asked
                        && !model.closing;
                    match result {
                        Some(Ok(answer)) if current => done(model, answer),
                        Some(Err(error)) if current => model.report(&error),
                        _ => {}
                    }
                    model.end_negative(request);
                    model.mark(dirty::INSPECTOR | dirty::STATUS);
                });
            })
            .ok();
    }

    /// Finds the film base (or reads it at `point`, fractions of the whole
    /// photograph), balances the print for it and meters the print
    /// exposure. `undoable`: its own undo step (Find again, a pick); else
    /// folded into the edit that asked. `crop`: then crops to the picture
    /// found, as its own undo step, unless the photograph is cropped.
    pub fn detect_negative_base(&mut self, point: Option<[f64; 2]>, undoable: bool, crop: bool) {
        if !self.is_negative() {
            return;
        }
        let generation = self.source_generation;
        if let Some(point) = point {
            self.negative.point = Some((generation, point));
        }
        let point = point.or_else(|| self.negative.point.filter(|(g, _)| *g == generation).map(|(_, p)| p));
        let asked = self.state.clone();
        let working = if point.is_none() { "Finding the film base…" } else { "Reading the base…" };
        self.run_negative(
            working,
            true,
            !undoable,
            move |asker| {
                let extra: Vec<(&str, Value)> = point.map(|p| ("point", json!(p))).into_iter().collect();
                let reply = asker.ask("negative_base", &[], &extra)?;
                if point.is_none() {
                    asker.note_survey(&reply);
                }
                let mut changes: Vec<(String, Value)> = vec![
                    ("film_render.base".into(), reply["base"].clone()),
                    ("film_render.convert.exposure_compensation_ev".into(), reply["exposure_ev"].clone()),
                    ("enlarger.m_filter_shift".into(), reply["m_filter_shift"].clone()),
                    ("enlarger.y_filter_shift".into(), reply["y_filter_shift"].clone()),
                ];
                // Each frame gets its own print exposure, as a printer exposes each frame.
                if let Some(exposure) = asker.meter(&changes) {
                    changes.push(("enlarger.print_exposure".into(), exposure));
                }
                Ok((reply, changes))
            },
            move |model, (reply, changes)| {
                let status = BaseStatus::of(&reply);
                model.record_base(reply["base"].clone(), status.clone(), generation);
                if undoable {
                    let mut next = model.state.clone();
                    for (path, value) in &changes {
                        json::set(&mut next.params, path, value.clone());
                    }
                    model.apply(next, if point.is_none() { "Detect Film Base" } else { "Pick Film Base" });
                } else {
                    model.fold_negative(changes, asked, generation);
                }
                model.status = status.sentence(model.state.orientation());
                if crop {
                    // Trying Your negative in the tray: crop once it's kept.
                    if model.tray().is_some() {
                        model.negative.crop_pending = Some(generation);
                    } else {
                        model.crop_to_found_picture();
                    }
                }
            },
        );
    }

    fn fold_negative(&mut self, changes: Vec<(String, Value)>, asked: EditState, generation: u64) {
        self.negative.folded.retain(|(g, a, _)| *g == generation && *a != asked);
        self.negative.folded.push((generation, asked, changes.clone()));
        if self.negative.folded.len() > 16 {
            self.negative.folded.remove(0);
        }
        let mut next = self.state.clone();
        for (path, value) in changes {
            json::set(&mut next.params, &path, value);
        }
        self.replace_state(next);
    }

    /// Undo or redo put back a negative's edit: one undone before its
    /// answer came gets it back; one whose base was never found finds it now.
    fn restore_negative_answer(&mut self) {
        if !self.is_negative() {
            return;
        }
        let generation = self.source_generation;
        if let Some((_, _, changes)) = self.negative.folded.iter().rev().find(|(g, a, _)| *g == generation && *a == self.state).cloned() {
            let mut next = self.state.clone();
            for (path, value) in changes {
                json::set(&mut next.params, &path, value);
            }
            self.replace_state(next);
        } else if neg::same_base(self.state.value("film_render.base"), json::get(&self.defaults, "film_render.base")) {
            self.detect_negative_base(None, false, false);
        }
    }

    /// After an edit (`replace_state`): entering negative mode, or a new
    /// film, scan light or white balance, finds the base and balance again;
    /// a new paper prints it as Auto does there. Both are part of that edit.
    /// Entering also crops to the picture, as a step of its own.
    pub(super) fn follow_negative(&mut self, old: &EditState, undoing: bool) {
        let Some(key) = neg::key(&self.state) else {
            if neg::key(old).is_some() {
                self.negative.pick = None;
                self.negative.crop_pending = None;
                // Back to a scene: Match camera brightness as for a new photograph.
                if self.state.brightness_match.is_none() && !undoing {
                    if let Some(found) = self.new_match() {
                        let mut next = self.state.clone();
                        next.brightness_match = Some(found);
                        self.replace_state(next);
                    }
                }
            }
            return;
        };
        if undoing {
            return self.restore_negative_answer();
        }
        let before = neg::key(old);
        if before.as_deref() != Some(key.as_str()) {
            self.detect_negative_base(None, false, before.is_none());
        } else if self.state.paper != old.paper {
            if self.negative.detecting && self.negative_busy() {
                // A detection still running balances for the old paper.
                self.detect_negative_base(None, false, false);
            } else if filtration(&self.state) == filtration(old) {
                self.auto_print_for_paper();
            }
        }
    }

    /// After a reset on a negative that put back its base, balance or print
    /// exposure (`paths`, None for all): found again as part of the reset.
    pub(super) fn refind_negative(&mut self, paths: Option<&[&str]>) {
        if !self.is_negative() || paths.is_some_and(|p| !p.iter().any(|p| neg::is_found(p))) {
            return;
        }
        self.detect_negative_base(None, false, false);
    }

    /// A new paper on the same base: printed as Auto prints it there,
    /// folded into the edit that changed the paper.
    fn auto_print_for_paper(&mut self) {
        let (asked, generation) = (self.state.clone(), self.source_generation);
        let decimal = crate::ui::inspector::decimal_separator();
        self.run_negative(
            "Printing like a minilab…",
            false,
            true,
            |asker| {
                let bounds = asker.bounds();
                let extra: Vec<(&str, Value)> = bounds.into_iter().map(|b| ("bounds", b)).collect();
                let reply = asker.ask("negative_auto", &[], &extra)?;
                Ok(["m_filter_shift", "y_filter_shift", "print_exposure"].map(|k| (format!("enlarger.{k}"), reply[k].clone())).to_vec())
            },
            move |model, changes| {
                model.status = balance_sentence(&changes, decimal);
                model.fold_negative(changes, asked, generation);
            },
        );
    }

    /// Prep › Find again, forgetting a picked point.
    pub fn redetect_negative_base(&mut self) {
        self.negative.point = None;
        self.detect_negative_base(None, true, false);
    }

    /// Prep › Auto: a minilab's print of this frame.
    pub fn auto_print_negative(&mut self) {
        self.negative_print("negative_auto", None, "Printing like a minilab…", "Auto Print");
    }

    /// Auto, or the neutral picker: the magenta and yellow, then the print
    /// exposure metered for them, as one undo step.
    fn negative_print(&mut self, name: &'static str, point: Option<[f64; 2]>, working: &str, action: &'static str) {
        if !self.is_negative() || self.negative_busy() {
            return;
        }
        let decimal = crate::ui::inspector::decimal_separator();
        self.run_negative(
            working,
            false,
            false,
            move |asker| {
                let mut extra: Vec<(&str, Value)> = point.map(|p| ("point", json!(p))).into_iter().collect();
                if point.is_none() {
                    extra.extend(asker.bounds().map(|b| ("bounds", b)));
                }
                let reply = asker.ask(name, &[], &extra)?;
                let mut changes: Vec<(String, Value)> =
                    vec![("enlarger.m_filter_shift".into(), reply["m_filter_shift"].clone()), ("enlarger.y_filter_shift".into(), reply["y_filter_shift"].clone())];
                let exposure = Some(reply["print_exposure"].clone()).filter(|v| !v.is_null()).or_else(|| asker.meter(&changes));
                changes.extend(exposure.map(|e| ("enlarger.print_exposure".to_string(), e)));
                Ok(changes)
            },
            move |model, changes| {
                let mut next = model.state.clone();
                for (path, value) in &changes {
                    json::set(&mut next.params, path, value.clone());
                }
                model.apply(next, action);
                model.status = balance_sentence(&changes, decimal);
            },
        );
    }

    /// Prep › Crop to picture: the picture inside the clear film, levelled
    /// by the rebate, as one undo step.
    pub fn crop_to_picture(&mut self) {
        if !self.is_negative() || self.negative_busy() || self.cropping() {
            return;
        }
        self.run_negative(
            "Finding the picture…",
            false,
            false,
            |asker| Ok(asker.survey()?.picture.clone()),
            |model, picture| match neg::crop(&picture) {
                Some(crop) if !model.cropping() => {
                    let mut next = model.state.clone();
                    next.crop = crop;
                    model.apply(next, "Crop to Picture");
                }
                Some(_) => {}
                None => model.status = "No clear film round the picture to crop to: crop it by hand.".into(),
            },
        );
    }

    /// The crop to the picture last found, as its own undo step, unless the
    /// photograph is cropped already (its crop is the user's).
    fn crop_to_found_picture(&mut self) {
        if !self.is_negative() || self.state.crop.cropped() || self.cropping() {
            return;
        }
        let Some(crop) = self.negative_survey().and_then(|s| neg::crop(&s.picture)) else { return };
        let mut next = self.state.clone();
        next.crop = crop;
        self.apply(next, "Crop to Picture");
    }

    /// A new photograph: nothing read on the last one applies.
    pub(super) fn forget_negative(&mut self) {
        self.negative.pick = None;
        self.negative.crop_pending = None;
        self.negative.point = None;
        self.negative.request.fetch_add(1, Ordering::Relaxed);
        self.negative.working = None;
        self.negative.detecting = false;
        self.negative.backlight_shown = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_balance_reads_as_the_macs() {
        let changes = vec![
            ("enlarger.m_filter_shift".to_string(), json!(4.43)),
            ("enlarger.y_filter_shift".to_string(), json!(-1.81)),
            ("enlarger.print_exposure".to_string(), json!(0.8712)),
        ];
        assert_eq!(balance_sentence(&changes, '.'), "Magenta 4.4, yellow −1.8, print exposure 0.87");
        assert_eq!(balance_sentence(&changes[..2], ','), "Magenta 4,4, yellow −1,8");
    }
}
