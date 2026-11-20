//! Compare and Original ⌄ (ticket 46; `Compare.swift`'s model side): two
//! photographs side by side or in one frame, or the open one against its
//! original. The open photograph is the one the panel edits, developed as
//! ever; the other is developed once from its sidecar, as opening it would
//! develop it, in a helper of its own once the open one has settled. Rules and geometry are in `crate::compare`; the view in
//! `ui::compare`.

use super::*;
use crate::compare::{self as rules, Comparison, Layout, Origin, OriginalMode, OriginalReference, Side};
use negs_library::Item;

/// The longest side a picture in the view keeps.
const MAX_SHOWN: u32 = 3072;

/// Pixels as shown: cropped, mirrored and turned, in sRGB (or Rec. 2020 if `wide`).
#[derive(Clone)]
pub struct Shown {
    pub rgba: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub wide: bool,
}

impl Shown {
    pub fn aspect(&self) -> f64 {
        self.width as f64 / self.height.max(1) as f64
    }

    /// `rgba` (width × height) cropped to `pixels` and straightened by the
    /// crop's angle (degrees, as the canvas turns it), mirrored, then turned.
    fn of(rgba: &[u8], width: u32, height: u32, pixels: Option<(crate::crop::Rect, f64)>, orientation: crate::crop::Orientation, wide: bool) -> Option<Shown> {
        // Read in place: a develop is 100 MB.
        let source = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(width, height, rgba)?;
        let image = match pixels {
            Some((r, angle)) if angle != 0.0 => straightened(&source, r, angle),
            Some((r, _)) => {
                let x = (r.x.round().max(0.0) as u32).min(width.saturating_sub(1));
                let y = (r.y.round().max(0.0) as u32).min(height.saturating_sub(1));
                let w = (r.width.round() as u32).clamp(1, width - x);
                let h = (r.height.round() as u32).clamp(1, height - y);
                let stride = width as usize * 4;
                let rows = (y as usize..(y + h) as usize).flat_map(|row| rgba[row * stride + x as usize * 4..][..w as usize * 4].iter().copied());
                image::RgbaImage::from_raw(w, h, rows.collect())?
            }
            None if width.max(height) > MAX_SHOWN && width.max(height) / 2 >= MAX_SHOWN * 3 / 4 => halved(&source),
            None => image::RgbaImage::from_raw(width, height, rgba.to_vec())?,
        };
        // Halved while that keeps three quarters of the size wanted: a
        // fraction of a filtered resize's time.
        let mut image = image;
        while image.width().max(image.height()) > MAX_SHOWN && image.width().max(image.height()) / 2 >= MAX_SHOWN * 3 / 4 {
            image = halved(&image);
        }
        let image = if orientation.flip { image::imageops::flip_horizontal(&image) } else { image };
        let image = match orientation.turns.rem_euclid(4) {
            1 => image::imageops::rotate90(&image),
            2 => image::imageops::rotate180(&image),
            3 => image::imageops::rotate270(&image),
            _ => image,
        };
        // Half a large screen at most; the view draws through GDI, 4096 at most.
        let (w, h) = image.dimensions();
        let image = if w.max(h) > MAX_SHOWN {
            let scale = MAX_SHOWN as f64 / w.max(h) as f64;
            image::imageops::resize(&image, ((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1), image::imageops::FilterType::Triangle)
        } else {
            image
        };
        let (width, height) = image.dimensions();
        Some(Shown { rgba: Arc::new(image.into_raw()), width, height, wide })
    }
}

/// `image` at half its size, each pixel the mean of four.
fn halved<C: std::ops::Deref<Target = [u8]>>(image: &image::ImageBuffer<image::Rgba<u8>, C>) -> image::RgbaImage {
    let (width, height) = image.dimensions();
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let source: &[u8] = image.as_raw();
    let stride = width as usize * 4;
    let mut out = vec![0u8; w as usize * h as usize * 4];
    for (y, row) in out.chunks_exact_mut(w as usize * 4).enumerate() {
        let y0 = (2 * y).min(height as usize - 1);
        let y1 = (2 * y + 1).min(height as usize - 1);
        for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
            let x0 = (2 * x).min(width as usize - 1) * 4;
            let x1 = (2 * x + 1).min(width as usize - 1) * 4;
            for c in 0..4 {
                let sum = source[y0 * stride + x0 + c] as u32 + source[y0 * stride + x1 + c] as u32 + source[y1 * stride + x0 + c] as u32 + source[y1 * stride + x1 + c] as u32;
                pixel[c] = ((sum + 2) / 4) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(w, h, out).expect("sized")
}

/// The crop `r` (pixels) of `image` straightened by `angle` degrees about
/// the photograph's centre, as the canvas's shader samples it: each pixel
/// of the crop reads the photograph turned back by the angle.
fn straightened<C: std::ops::Deref<Target = [u8]>>(image: &image::ImageBuffer<image::Rgba<u8>, C>, r: crate::crop::Rect, angle: f64) -> image::RgbaImage {
    let (width, height) = image.dimensions();
    let (w, h) = ((r.width.round() as u32).max(1), (r.height.round() as u32).max(1));
    let (s, k) = angle.to_radians().sin_cos();
    let (cx, cy) = (width as f64 / 2.0, height as f64 / 2.0);
    let (max_x, max_y) = ((width.max(1) - 1) as f64, (height.max(1) - 1) as f64);
    image::RgbaImage::from_fn(w, h, |x, y| {
        let (dx, dy) = (r.x + x as f64 + 0.5 - cx, r.y + y as f64 + 0.5 - cy);
        let (gx, gy) = (cx + dx * k + dy * s - 0.5, cy - dx * s + dy * k - 0.5);
        image::imageops::interpolate_bilinear(image, gx.clamp(0.0, max_x) as f32, gy.clamp(0.0, max_y) as f32).unwrap_or(image::Rgba([0, 0, 0, 255]))
    })
}

#[derive(Default)]
pub struct Memory {
    pub comparison: Option<Comparison>,
    /// The photograph that isn't open (or isn't the original), and its
    /// develop once in.
    other: Option<(Item, Option<Shown>)>,
    /// Counts the other's develops: a late one for another is dropped.
    request: u64,
    /// The open photograph as last shown, kept for the side it moves to
    /// when the other is clicked.
    kept: Option<(Item, Shown)>,
    /// What the open photograph's picture was shown from: its id and edit.
    shown_key: Option<(u64, u64, String)>,
    shown: Option<Shown>,
    /// What the picture being made off the UI thread is made from.
    making: Option<(u64, u64, String)>,
    /// The original as shown, with what it was made from: the decode's id
    /// and the crop and turn; and what the one being made is made from.
    original_shown: Option<((u64, String), Shown)>,
    making_original: Option<(u64, String)>,
    /// What the last context menu offered.
    offered: Option<rules::Offer>,
    /// The other photograph's develop, once the edited one has settled.
    waiting: Option<Waiting>,
    /// Compare's own helper, so the open photograph's decode stays.
    engine: Option<Arc<EngineClient>>,
}

/// A develop of the other photograph waiting its turn.
struct Waiting {
    item: Item,
    state: EditState,
    needs_match: bool,
    request: u64,
}

impl Model {
    pub fn comparison(&self) -> Option<&Comparison> {
        self.compare.comparison.as_ref()
    }

    pub fn can_compare(&self) -> bool {
        self.source.is_some() && self.has_catalog() && !self.cropping() && self.shelf.library.is_some()
    }

    /// The photograph's number on the roll (7, 7a).
    pub fn frame_number(&self, item: &Item) -> String {
        self.shelf.library.as_ref().map_or_else(String::new, |l| rules::frame_number(&l.items(), item))
    }

    /// The selection in shown order.
    fn compare_selection(&self) -> Vec<Item> {
        self.shelf.selection.ordered(&self.shown_items())
    }

    /// The title bar's Compare word, the C key and View › Compare.
    pub fn compare_word(&self) -> (bool, String) {
        rules::word(&self.compare_selection(), &|i| self.frame_number(i), self.can_compare())
    }

    pub fn can_toggle_compare(&self) -> bool {
        self.comparison().is_some() || self.compare_word().0
    }

    /// C and View › Compare: in compare it leaves; else it compares the two
    /// selected. The UI closes the grid; `origin` is where it was entered.
    pub fn toggle_compare(&mut self, origin: Origin) {
        if self.comparison().is_some() {
            return self.leave_compare();
        }
        if let [first, second] = self.compare_selection().as_slice() {
            self.compare(first.clone(), second.clone(), origin);
        }
    }

    /// Compares two photographs, `first` left or on top. The panel edits
    /// the open photograph if it's one of them, else the first, opened.
    pub fn compare(&mut self, first: Item, second: Item, origin: Origin) {
        if !self.can_compare() || first == second || self.comparison().is_some() {
            return;
        }
        let current = if origin == Origin::Grid { self.shelf.selection.current().cloned() } else { self.library_item() };
        let edited = if current.as_ref() == Some(&second) { second.clone() } else { first.clone() };
        self.begin_compare(Comparison::new(Side::Photo(first.clone()), Side::Photo(second), edited == first, origin), edited);
    }

    /// The open photograph against its original: developed first, side by
    /// side; on a split line the original is left of the line, as on the
    /// Mac's (`startSplit`).
    pub fn compare_with_original(&mut self, layout: Layout) {
        let Some(item) = self.library_item() else { return };
        if !self.can_compare() || self.comparison().is_some() {
            return;
        }
        let (photo, original) = (Side::Photo(item.clone()), Side::Original(item.clone()));
        let mut comparison =
            if layout == Layout::Slider { Comparison::new(original, photo, false, Origin::Photo) } else { Comparison::new(photo, original, true, Origin::Photo) };
        comparison.layout = layout;
        self.begin_compare(comparison, item);
    }

    fn begin_compare(&mut self, comparison: Comparison, edited: Item) {
        self.end_tray(true);
        self.hold_original(false);
        let original = comparison.against_original();
        self.compare.comparison = Some(comparison);
        if self.library_item().as_ref() != Some(&edited) {
            self.open_item(edited);
        }
        if original {
            self.prepare_original();
        }
        self.prepare_compared();
        self.mark(dirty::LAYOUT | dirty::CANVAS | dirty::CHROME | dirty::STATUS | dirty::COMPARE);
    }

    /// Done, Esc or C: back to where compare was entered.
    pub fn leave_compare(&mut self) {
        if self.compare.comparison.take().is_none() {
            return;
        }
        self.compare.other = None;
        self.compare.kept = None;
        self.compare.original_shown = None;
        self.compare.making_original = None;
        self.compare.request += 1;
        self.stop_compare_engine();
        // Holding Original needs only the texture.
        if let Some((_, picture)) = &self.original_cache {
            picture.release_kept();
        }
        self.mark(dirty::LAYOUT | dirty::CANVAS | dirty::CHROME | dirty::STATUS | dirty::COMPARE);
    }

    /// S: side by side ↔ slider.
    pub fn set_compare_layout(&mut self, layout: Layout) {
        if let Some(c) = &mut self.compare.comparison {
            c.layout = layout;
            self.mark(dirty::COMPARE | dirty::STATUS);
        }
    }

    pub fn toggle_compare_layout(&mut self) {
        if let Some(layout) = self.comparison().map(|c| c.layout) {
            self.set_compare_layout(if layout == Layout::SideBySide { Layout::Slider } else { Layout::SideBySide });
        }
    }

    pub fn move_split(&mut self, fraction: f64) {
        if let Some(c) = &mut self.compare.comparison {
            c.split = fraction.clamp(0.0, 1.0);
            self.mark(dirty::COMPARE);
        }
    }

    /// Swap: the two change places; the same photograph stays edited.
    pub fn swap_compared(&mut self) {
        if let Some(c) = &mut self.compare.comparison {
            std::mem::swap(&mut c.first, &mut c.second);
            c.active_first = !c.active_first;
            self.mark(dirty::COMPARE | dirty::STATUS | dirty::CHROME);
        }
    }

    /// A click on a photograph edits it instead; the one edited until now
    /// keeps its last develop on its side.
    pub fn activate_compared(&mut self, first: bool) {
        let Some(c) = &mut self.compare.comparison else { return };
        if c.active_first == first {
            return;
        }
        c.active_first = first;
        let side = if first { c.first.clone() } else { c.second.clone() };
        self.mark(dirty::COMPARE | dirty::STATUS);
        let Side::Photo(item) = side else { return };
        if self.library_item().as_ref() == Some(&item) {
            return;
        }
        // The one open until now keeps its picture; the other's is the open one's stand-in.
        let open = self.library_item();
        let shown = self.compared_open_now();
        let other = self.compare.other.take();
        if let (Some(open), Some(shown)) = (open, shown) {
            self.compare.kept = Some((open.clone(), shown.clone()));
            self.compare.other = Some((open, Some(shown)));
        }
        if let Some((_, Some(picture))) = other {
            self.compare.shown = Some(picture);
            self.compare.shown_key = None;
        }
        self.open_item(item);
        self.prepare_compared();
    }

    /// ← →: the second photograph steps through the shown ones, skipping the first.
    pub fn step_compared(&mut self, offset: isize) {
        let Some(c) = self.comparison().cloned() else { return };
        if c.against_original() {
            return;
        }
        let Some(next) = rules::step(c.second.item(), c.first.item(), &self.shown_items(), offset) else { return };
        let edited_stepped = !c.active_first;
        if let Some(c) = &mut self.compare.comparison {
            c.second = Side::Photo(next.clone());
        }
        if edited_stepped {
            self.compare.shown = None;
            self.open_item(next);
        }
        self.prepare_compared();
        self.mark(dirty::COMPARE | dirty::STATUS);
    }

    /// The side that isn't open: the original, or another photograph to develop.
    fn prepare_compared(&mut self) {
        let Some(c) = self.comparison().cloned() else { return };
        let side = c.inactive().clone();
        let Side::Photo(item) = side else { return };
        if item == *c.edited() || self.compare.other.as_ref().is_some_and(|(i, _)| *i == item) {
            return;
        }
        let kept = self.compare.kept.as_ref().filter(|(i, _)| *i == item).map(|(_, s)| s.clone());
        self.compare.other = Some((item.clone(), kept.clone()));
        self.compare.request += 1;
        let request = self.compare.request;
        let Some((state, needs_match)) = self.compared_state(&item) else { return };
        if kept.is_none() {
            self.load_compared_stand_in(&item, &state, request);
        }
        self.compare.waiting = Some(Waiting { item, state, needs_match, request });
        self.pump_compared();
    }

    /// `item`'s edits as opening it would restore them, or a new
    /// photograph's (`comparedState`), and whether its match is to be
    /// solved: wanted, possible, and not solved for these edits. The same
    /// edits as opening it, so its solve is the one opening it looks up.
    fn compared_state(&self, item: &Item) -> Option<(EditState, bool)> {
        let library = self.shelf.library.as_ref()?;
        let limit = self.state.limit_preview;
        let raw = is_raw(&item.photo);
        let matchable = |s: &EditState| s.brightness_match.is_some() && (raw || json::string(s.value("io.input_interpretation.mode")) == "auto");
        let (stored, solved) = match library.export_edits(item) {
            Some(stored) => (stored.state, stored.match_solved),
            None => {
                let previous = library.state().cloned().unwrap_or_else(negs_library::EditState::empty);
                (library.defaults().new_photo_state(&previous), false)
            }
        };
        let mut state = library_map::from_library(&stored, limit).ok()?;
        state.params = self.restored_session_params(&state.params);
        let state = self.with_preferences(state);
        let needs = !solved && matchable(&state);
        Some((state, needs))
    }

    /// The other photograph's stand-in at once, as opening it would show
    /// one (`load(_ other:)`): its last develop if kept, else its developed
    /// thumbnail, else the camera's preview.
    fn load_compared_stand_in(&self, item: &Item, state: &EditState, request: u64) {
        let (kept, thumbnail) = (self.kept_key(item), self.filed_key(crate::edited_store::thumbnails(), item));
        let (crop, orientation, photo, item) = (state.crop, state.orientation(), item.photo.clone(), item.clone());
        // A develop to an Image Size is cropped by the helper already.
        let cropped_render = state::size_mode(&state.params) == state::SizeMode::LongestSide && crop.cropped();
        // Pictures leave the CPU once on the GPU: these are read as pixels.
        let as_shown = move |(rgba, width, height): (Vec<u8>, u32, u32), wide: bool, crop_it: bool| {
            let pixels = (crop_it && crop.cropped()).then(|| (crop.pixels(Size::new(width as f64, height as f64)), crop.angle));
            Shown::of(&rgba, width, height, pixels, orientation, wide)
        };
        #[cfg(test)]
        let background = ui::testing::background();
        std::thread::spawn(move || {
            #[cfg(test)]
            let _background = background;
            let shown = kept
                .and_then(|(key, loose)| KeptDevelop::pixels(&key, loose.as_deref()))
                .and_then(|(rgba, w, h, space, _)| as_shown((rgba, w, h), crate::picture::is_wide(&space), !cropped_render))
                .map(|shown| (shown, "kept"))
                .or_else(|| {
                    // Finished: cropped, turned and bordered.
                    let (rgba, w, h) = thumbnail.and_then(|(key, loose)| stand_in_pixels(&key, loose.as_deref()))?;
                    Some((Shown::of(&rgba, w, h, None, Default::default(), false)?, "thumbnail"))
                })
                .or_else(|| {
                    let (rgba, w, h, _) = crate::thumbnail::placeholder_pixels(&photo)?;
                    Some((as_shown((rgba, w, h), false, true)?, "embedded"))
                });
            crate::trace::event("compare_stand_in", || json!({"request": request, "shown": shown.as_ref().map(|(_, kind)| *kind)}));
            let Some((shown, _)) = shown else { return };
            ui::post(move |app: &mut App| {
                let model = &mut app.model;
                if request != model.compare.request {
                    return;
                }
                // Never over the develop or the open one's last picture.
                if let Some((other, picture @ None)) = &mut model.compare.other
                    && *other == item
                {
                    *picture = Some(shown);
                    model.mark(dirty::COMPARE);
                }
            });
        });
    }

    /// Develops the other photograph once the edited one has settled, never
    /// two develops at once (`scheduleCompareDevelop`): in a helper of its
    /// own, the match solved first as opening it would (the match cache
    /// first), then a full preview `render` as the photo view's.
    pub(super) fn pump_compared(&mut self) {
        if self.compare.comparison.is_none() || !self.preview_current || self.matching_brightness || self.compare.waiting.is_none() {
            return;
        }
        let Some(Waiting { item, state, needs_match, request }) = self.compare.waiting.take() else { return };
        if request != self.compare.request {
            return;
        }
        let Some(paths) = self.engine_paths().cloned() else { return };
        let engine = match &self.compare.engine {
            Some(engine) => engine.clone(),
            None => match EngineClient::start(paths) {
                Ok(engine) => {
                    let engine = Arc::new(engine);
                    self.compare.engine = Some(engine.clone());
                    engine
                }
                Err(error) => return self.report(&error),
            },
        };
        let solving = self.solving();
        let space = self.preview_space().to_string();
        crate::trace::event("compare_develop", || json!({"item": item.photo, "request": request, "match": needs_match}));
        #[cfg(test)]
        let background = ui::testing::background();
        std::thread::spawn(move || {
            #[cfg(test)]
            let _background = background;
            let generation = 2_000_000_000 + request;
            // Unmatched if the solve fails, as the photograph would open.
            let state = if needs_match {
                super::batch::solve_match(&engine, &item.photo, &state, true, &solving, generation, "compare").map_or(state, |(solved, _)| solved)
            } else {
                state
            };
            let mut rendered = state.clone();
            json::set(&mut rendered.params, "io.output_color_space", json!(space));
            let buffer = engine.directory().join(format!("compare-{request}.rgba"));
            let mut command = json!({
                "command": "render", "input": item.photo, "source_generation": generation,
                "film": rendered.film, "paper": rendered.paper, "params": rendered.params,
                "limit_preview": rendered.limit_preview, "preview_path": buffer,
            });
            let crop = state.crop;
            let cropped_render = state::size_mode(&state.params) == state::SizeMode::LongestSide && crop.cropped();
            if cropped_render {
                let r = crop.rect;
                command["crop"] = json!({"x": r.x, "y": r.y, "width": r.width, "height": r.height, "angle": crop.angle});
            }
            let (sender, receiver) = std::sync::mpsc::channel();
            engine.request(command, engine::timeout_for("render"), move |result| {
                let _ = sender.send(result);
            });
            let result = receiver.recv().map_err(|_| anyhow!("The helper stopped")).and_then(|r| r);
            crate::trace::event("compare_developed", || match &result {
                Ok((_, r)) => json!({"request": request, "width": r["width"], "height": r["height"], "matched": state.brightness_match.as_ref().and_then(|m| m.ev)}),
                Err(error) => json!({"request": request, "error": format!("{error:#}")}),
            });
            let shown = result.ok().and_then(|(_, r)| {
                let rgba = std::fs::read(&buffer).ok()?;
                let (w, h) = (json::number(&r["width"]) as u32, json::number(&r["height"]) as u32);
                let pixels = (!cropped_render && crop.cropped()).then(|| (crop.pixels(Size::new(w as f64, h as f64)), crop.angle));
                Shown::of(&rgba, w, h, pixels, state.orientation(), crate::picture::is_wide(json::string(&r["display_color_space"])))
            });
            let _ = std::fs::remove_file(&buffer);
            ui::post(move |app: &mut App| app.model.compared_developed(request, item, shown));
        });
    }

    fn compared_developed(&mut self, request: u64, item: Item, shown: Option<Shown>) {
        if request != self.compare.request {
            return self.pump_compared();
        }
        if let Some((other, picture)) = &mut self.compare.other
            && *other == item
            && shown.is_some()
        {
            *picture = shown;
            self.mark(dirty::COMPARE);
        }
        self.pump_compared();
    }

    /// The commands Compare's own helper was sent, in order.
    #[cfg(test)]
    pub fn compare_commands(&self) -> Vec<String> {
        self.compare.engine.as_ref().map_or_else(Vec::new, |e| e.commands.lock().unwrap().iter().map(|c| json::string(&c["command"]).to_string()).collect())
    }

    /// Compare's helper, once compare is left.
    fn stop_compare_engine(&mut self) {
        self.compare.waiting = None;
        if let Some(engine) = self.compare.engine.take() {
            std::thread::spawn(move || engine.stop());
        }
    }

    /// The open photograph as shown: its settled develop (the preview's
    /// pixels leave memory once on the GPU), cropped, mirrored and turned.
    /// Made off the UI thread (a develop is cropped, turned and scaled down
    /// in about a second): the picture last made shows until then.
    pub fn compared_open(&mut self) -> Option<Shown> {
        let Some((key, job)) = self.open_shown_job() else { return self.compare.shown.clone() };
        if self.compare.making.as_ref() != Some(&key) {
            self.compare.making = Some(key.clone());
            #[cfg(test)]
            let background = ui::testing::background();
            std::thread::spawn(move || {
                #[cfg(test)]
                let _background = background;
                let shown = job();
                ui::post(move |app: &mut App| app.model.compared_open_made(key, shown));
            });
        }
        self.compare.shown.clone()
    }

    /// `compared_open` at once, for a photograph about to be closed.
    fn compared_open_now(&mut self) -> Option<Shown> {
        if let Some((key, job)) = self.open_shown_job() {
            self.compared_open_made(key, job());
        }
        self.compare.shown.clone()
    }

    /// What the open photograph's picture would be made from now, and the
    /// making; None while the one shown is current.
    fn open_shown_job(&self) -> Option<((u64, u64, String), impl FnOnce() -> Option<Shown> + Send + 'static)> {
        let orientation = self.state.orientation();
        let crop = self.state.crop;
        let cropped_render = self.size_mode() == state::SizeMode::LongestSide && crop.cropped();
        let kept = self.kept.as_ref().filter(|k| k.generation == self.source_generation)?;
        let key = (kept.generation, kept.revision, format!("{crop:?}{orientation:?}"));
        if self.compare.shown_key.as_ref() == Some(&key) {
            return None;
        }
        let pixels = (!cropped_render && crop.cropped()).then(|| (crop.pixels(Size::new(kept.width as f64, kept.height as f64)), crop.angle));
        let (rgba, width, height, wide) = (kept.pixels.clone(), kept.width, kept.height, crate::picture::is_wide(&kept.space));
        Some((key, move || Shown::of(&rgba, width, height, pixels, orientation, wide)))
    }

    fn compared_open_made(&mut self, key: (u64, u64, String), shown: Option<Shown>) {
        if self.compare.making.as_ref() == Some(&key) {
            self.compare.making = None;
        }
        if shown.is_some() && self.compare.comparison.is_some() {
            self.compare.shown_key = Some(key);
            self.compare.shown = shown;
            self.mark(dirty::COMPARE);
        }
    }

    /// What the original's side says while nothing is shown on it yet
    /// (`loadingOriginal`): Loading original, unless its decode failed.
    pub fn compared_original_loading(&self) -> Option<&'static str> {
        let against = self.comparison().is_some_and(|c| c.against_original());
        (against && !self.original_failed).then(|| self.loading_original_title())
    }

    /// The original as shown beside the open photograph: cropped, mirrored
    /// and turned from the decode's pixels off the UI thread, once for each
    /// crop and turn, like `compared_open`. Until it's made, the last one
    /// made from the same decode shows.
    pub fn compared_original(&mut self) -> Option<Shown> {
        let (_, picture) = self.original_cache.as_ref()?;
        let (crop, orientation) = (self.state.crop, self.state.orientation());
        let key = (picture.id, format!("{crop:?}{orientation:?}"));
        let last = self.compare.original_shown.as_ref().filter(|((id, _), _)| *id == picture.id);
        if last.is_some_and(|(made, _)| *made == key) || self.compare.making_original.as_ref() == Some(&key) {
            return last.map(|(_, shown)| shown.clone());
        }
        let last = last.map(|(_, shown)| shown.clone());
        self.compare.making_original = Some(key.clone());
        let picture = picture.clone();
        let pixels = crop.cropped().then(|| (crop.pixels(Size::new(picture.width as f64, picture.height as f64)), crop.angle));
        #[cfg(test)]
        let background = ui::testing::background();
        std::thread::spawn(move || {
            #[cfg(test)]
            let _background = background;
            let shown = picture.with_pixels(|rgba| Shown::of(rgba, picture.width, picture.height, pixels, orientation, picture.wide)).flatten();
            ui::post(move |app: &mut App| app.model.compared_original_made(key, shown));
        });
        last
    }

    fn compared_original_made(&mut self, key: (u64, String), shown: Option<Shown>) {
        if self.compare.making_original.as_ref() == Some(&key) {
            self.compare.making_original = None;
        }
        if shown.is_some() && self.comparison().is_some_and(|c| c.against_original()) {
            self.compare.original_shown = shown.map(|shown| (key, shown));
            self.mark(dirty::COMPARE);
        }
    }

    /// The other photograph's develop, once in.
    pub fn compared_other(&self) -> Option<Shown> {
        self.compare.other.as_ref().and_then(|(_, s)| s.clone())
    }

    /// A side's picture: the open photograph's, its original's or the other's.
    pub fn compared_side(&mut self, side: &Side) -> Option<Shown> {
        match side {
            Side::Original(_) => self.compared_original(),
            Side::Photo(item) if self.library_item().as_ref() == Some(item) => self.compared_open(),
            Side::Photo(_) => self.compared_other(),
        }
    }

    // MARK: Original ⌄

    pub fn original_mode(&self) -> OriginalMode {
        OriginalMode::from_key(&self.prefs.original_mode)
    }

    pub fn set_original_mode(&mut self, mode: OriginalMode) {
        self.prefs.original_mode = mode.key().into();
        self.prefs.save();
        self.mark(dirty::STATUS);
    }

    /// The RAW's camera JPEG (RAW+JPEG), when there is one.
    pub fn camera_jpeg(&self) -> Option<PathBuf> {
        let source = self.source.as_ref().filter(|_| self.is_raw)?;
        self.shelf.library.as_ref()?.companion(source).map(Path::to_path_buf)
    }

    pub fn original_reference(&self) -> OriginalReference {
        OriginalReference::from_key(&self.prefs.original_reference)
    }

    pub fn set_original_reference(&mut self, reference: OriginalReference) {
        if self.original_reference() == reference {
            return;
        }
        self.prefs.original_reference = reference.key().into();
        self.prefs.save();
        // The original decodes again, from the other file.
        self.original_cache = None;
        self.original = None;
        self.compare.original_shown = None;
        self.compare.making_original = None;
        self.mark(dirty::STATUS | dirty::CANVAS | dirty::COMPARE);
        if self.comparison().is_some_and(|c| c.against_original()) {
            self.prepare_original();
        }
    }

    /// Original shows the camera's JPEG rather than the RAW.
    pub fn shows_camera_jpeg(&self) -> bool {
        self.original_reference() == OriginalReference::CameraJpeg && self.camera_jpeg().is_some()
    }

    /// "Original" or "Camera JPEG".
    pub fn original_word(&self) -> &'static str {
        if self.shows_camera_jpeg() { "Camera JPEG" } else { "Original" }
    }

    /// A click on Original in Split or Side by side mode: in and out of
    /// compare with the original.
    pub fn toggle_original_compare(&mut self) {
        if self.comparison().is_some_and(|c| c.against_original()) {
            return self.leave_compare();
        }
        let layout = if self.original_mode() == OriginalMode::Split { Layout::Slider } else { Layout::SideBySide };
        self.compare_with_original(layout);
    }

    /// A photograph's film and paper ("Kodak Gold 200", "Kodak Professional
    /// Portra Endura"), from its sidecar unless it's open; no paper when the
    /// film is scanned.
    pub fn compared_stocks(&self, item: &Item) -> (String, Option<String>) {
        let name = |id: &str, paper: bool| self.stocks.iter().find(|s| s.paper == paper && s.id == id).map_or_else(|| id.to_string(), |s| s.name.clone());
        let (film, paper, scanning) = if self.library_item().as_ref() == Some(item) {
            (self.state.film.clone(), self.state.paper.clone(), json::boolean(self.value("io.scan_film")))
        } else {
            let Some(library) = &self.shelf.library else { return Default::default() };
            let (stored, _) = library.thumbnail_state(item).unwrap_or_else(|| (library.unedited_state(), false));
            let Ok(state) = library_map::from_library(&stored, false) else { return Default::default() };
            let scanning = json::boolean(state.value("io.scan_film"));
            (state.film, state.paper, scanning)
        };
        (name(&film, false), (!scanning).then(|| name(&paper, true)))
    }

    /// The context menu's Compare for a right-click on `item`: the selection
    /// it's in, or itself.
    pub fn compare_offer(&self, item: &Item, in_grid: bool) -> rules::Offer {
        let selected = self.compare_selection();
        let targets = if selected.contains(item) { selected } else { vec![item.clone()] };
        let current = if in_grid { self.shelf.selection.current().cloned() } else { self.library_item() };
        let with_original = !in_grid && self.library_item().as_ref() == Some(item) && self.image.is_some();
        rules::offer(&targets, current.as_ref(), item, &|i| self.frame_number(i), self.can_compare(), with_original, self.original_word())
    }

    /// The offer the context menu showed, for its command.
    pub fn offer_compare(&mut self, offer: rules::Offer) {
        self.compare.offered = Some(offer);
    }

    /// Acts on the offer the context menu showed; true if compare began.
    pub fn take_compare_offer(&mut self, origin: Origin) -> bool {
        let Some(offer) = self.compare.offered.take().filter(|o| o.enabled) else { return false };
        match offer.pair {
            Some((first, second)) => self.compare(first, second, origin),
            None if offer.with_original => {
                let layout = if self.original_mode() == OriginalMode::Split { Layout::Slider } else { Layout::SideBySide };
                self.compare_with_original(layout)
            }
            None => {}
        }
        self.comparison().is_some()
    }

    /// The title bar's name while comparing two photographs: "A.NEF and B.NEF".
    pub fn compare_title(&self) -> Option<String> {
        let c = self.comparison()?;
        let (Side::Photo(a), Side::Photo(b)) = (&c.first, &c.second) else { return None };
        Some(format!("{} and {}", super::file_name(&a.photo), super::file_name(&b.photo)))
    }

    /// The status line's words in compare: the two, by number and stock.
    pub fn compare_summary(&self) -> String {
        let Some(c) = self.comparison() else { return String::new() };
        let name = |side: &Side| match side {
            Side::Original(_) => self.original_word().to_string(),
            Side::Photo(item) => self.frame_number(item),
        };
        // The photograph first, wherever its original is.
        let (a, b) = if c.first.is_original() { (&c.second, &c.first) } else { (&c.first, &c.second) };
        format!("Comparing {} and {}", name(a), name(b))
    }
}

/// The camera's JPEG laid over the RAW's frame (`width` × `height`), so it
/// lines up with the developed image: one of the same shape (within 1%) is
/// stretched to it, another sits centred on black.
pub fn over_frame(rgba: &[u8], width: u32, height: u32, frame: (u32, u32)) -> Option<(Vec<u8>, u32, u32)> {
    let picture = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let aspect = frame.0 as f64 / frame.1.max(1) as f64;
    let long = width.max(height) as f64;
    let out = if aspect >= 1.0 { (long as u32, (long / aspect).round() as u32) } else { ((long * aspect).round() as u32, long as u32) };
    let same = ((width as f64 / height as f64) / aspect - 1.0).abs() < 0.01;
    if same {
        let resized = image::imageops::resize(&picture, out.0.max(1), out.1.max(1), image::imageops::FilterType::Triangle);
        return Some((resized.into_raw(), out.0, out.1));
    }
    let scale = (out.0 as f64 / width as f64).min(out.1 as f64 / height as f64);
    let (w, h) = (((width as f64 * scale).round() as u32).max(1), ((height as f64 * scale).round() as u32).max(1));
    let resized = image::imageops::resize(&picture, w, h, image::imageops::FilterType::Triangle);
    let mut canvas = image::RgbaImage::from_pixel(out.0.max(1), out.1.max(1), image::Rgba([0, 0, 0, 255]));
    image::imageops::overlay(&mut canvas, &resized, ((out.0 - w.min(out.0)) / 2) as i64, ((out.1 - h.min(out.1)) / 2) as i64);
    Some((canvas.into_raw(), out.0, out.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The open side's crop is straightened as the canvas shows it: a half
    /// turn about the photograph's centre reads the opposite corner.
    #[test]
    fn compare_straightens_the_crop() {
        let (w, h) = (4u32, 2u32);
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [i as u8 * 10, 0, 0, 255]).collect();
        let rect = crate::crop::Rect { x: 0.0, y: 0.0, width: 2.0, height: 1.0 };
        let plain = Shown::of(&rgba, w, h, Some((rect, 0.0)), Default::default(), false).unwrap();
        assert_eq!((plain.width, plain.height, plain.rgba[0], plain.rgba[4]), (2, 1, 0, 10));
        let turned = Shown::of(&rgba, w, h, Some((rect, 180.0)), Default::default(), false).unwrap();
        // The top left 2 × 1 read through a half turn: the bottom right, backwards.
        assert_eq!((turned.width, turned.height), (2, 1));
        let near = |v: u8, want: u8| (v as i32 - want as i32).abs() <= 1;
        assert!(near(turned.rgba[0], 70) && near(turned.rgba[4], 60), "{:?}", &turned.rgba[..8]);
    }

    /// A develop larger than the view keeps is halved, each pixel the mean
    /// of four, while that keeps three quarters of the size; then filtered.
    #[test]
    fn compare_halves_a_large_develop() {
        let (w, h) = (MAX_SHOWN * 2, 4u32);
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [if i % 2 == 0 { 100 } else { 201 }, 0, 0, 255]).collect();
        let shown = Shown::of(&rgba, w, h, None, Default::default(), false).unwrap();
        assert_eq!((shown.width, shown.height), (MAX_SHOWN, 2));
        assert_eq!(&shown.rgba[..4], &[151, 0, 0, 255]);
        let cropped = Shown::of(&rgba, w, h, Some((crate::crop::Rect { x: 1.0, y: 0.0, width: (w - 2) as f64, height: 4.0 }, 0.0)), Default::default(), false).unwrap();
        assert_eq!((cropped.width, cropped.height, cropped.rgba[0]), (MAX_SHOWN - 1, 2, 151));
        let (w, h) = (MAX_SHOWN + 100, 2u32);
        let shown = Shown::of(&vec![9u8; (w * h * 4) as usize], w, h, None, Default::default(), false).unwrap();
        assert_eq!((shown.width, shown.rgba[0]), (MAX_SHOWN, 9));
    }
}
