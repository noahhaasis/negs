//! Compare and Original ⌄ (ticket 46; `Compare.swift`'s model side): two
//! photographs side by side or in one frame, or the open one against its
//! original. The open photograph is the one the panel edits, developed as
//! ever; the other is developed once from its sidecar, at the canvas's
//! size. Rules and geometry are in `crate::compare`; the view in
//! `ui::compare`.

use super::*;
use crate::compare::{self as rules, Comparison, Layout, Origin, OriginalMode, OriginalReference, Side};
use negs_library::Item;

/// The other photograph's long side: sharp on a large screen's half.
const OTHER_LONG_SIDE: u32 = 2048;
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

    /// `rgba` (width × height) cropped to `pixels`, mirrored, then turned.
    fn of(rgba: &[u8], width: u32, height: u32, pixels: Option<crate::crop::Rect>, orientation: crate::crop::Orientation, wide: bool) -> Option<Shown> {
        let image = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
        let image = match pixels {
            Some(r) => {
                let x = (r.x.round().max(0.0) as u32).min(width.saturating_sub(1));
                let y = (r.y.round().max(0.0) as u32).min(height.saturating_sub(1));
                let w = (r.width.round() as u32).clamp(1, width - x);
                let h = (r.height.round() as u32).clamp(1, height - y);
                image::imageops::crop_imm(&image, x, y, w, h).to_image()
            }
            None => image,
        };
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
    /// What the last context menu offered.
    offered: Option<rules::Offer>,
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

    /// The open photograph against its original: developed first.
    pub fn compare_with_original(&mut self, layout: Layout) {
        let Some(item) = self.library_item() else { return };
        if !self.can_compare() || self.comparison().is_some() {
            return;
        }
        let mut comparison = Comparison::new(Side::Photo(item.clone()), Side::Original(item.clone()), true, Origin::Photo);
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
        self.compare.request += 1;
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
            self.mark(dirty::COMPARE | dirty::STATUS);
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
        let shown = self.compared_open();
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
        self.compare.other = Some((item.clone(), kept));
        self.develop_compared(item);
    }

    fn develop_compared(&mut self, item: Item) {
        let (Some(engine), Some(library)) = (self.engine.clone(), self.shelf.library.as_ref()) else { return };
        let (stored, _) = library.thumbnail_state(&item).unwrap_or_else(|| (library.unedited_state(), false));
        let Ok(mut state) = library_map::from_library(&stored, false) else { return };
        json::set(&mut state.params, "io.output_color_space", json!("sRGB"));
        self.compare.request += 1;
        let request = self.compare.request;
        let buffer = engine.directory().join(format!("compare-{request}.rgba"));
        let mut command = json!({
            "command": "thumbnail", "input": item.photo, "film": state.film, "paper": state.paper, "params": state.params,
            "long_side": OTHER_LONG_SIDE, "source_generation": 1_000_000 + request, "preview_path": buffer,
        });
        if state.crop.cropped() {
            let r = state.crop.rect;
            command["crop"] = json!({"x": r.x, "y": r.y, "width": r.width, "height": r.height, "angle": state.crop.angle});
        }
        let orientation = state.orientation();
        #[cfg(test)]
        let background = ui::testing::background();
        engine.request(command, engine::timeout_for("thumbnail"), move |result| {
            #[cfg(test)]
            let _background = background;
            let shown = result.ok().and_then(|(_, r)| {
                let rgba = std::fs::read(&buffer).ok()?;
                Shown::of(&rgba, json::number(&r["width"]) as u32, json::number(&r["height"]) as u32, None, orientation, false)
            });
            let _ = std::fs::remove_file(&buffer);
            ui::post(move |app: &mut App| app.model.compared_developed(request, item, shown));
        });
    }

    fn compared_developed(&mut self, request: u64, item: Item, shown: Option<Shown>) {
        if request != self.compare.request {
            return;
        }
        if let Some((other, picture)) = &mut self.compare.other
            && *other == item
            && shown.is_some()
        {
            *picture = shown;
            self.mark(dirty::COMPARE);
        }
    }

    /// The open photograph as shown: its preview cropped, mirrored and turned.
    pub fn compared_open(&mut self) -> Option<Shown> {
        let image = self.image.clone()?;
        let orientation = self.state.orientation();
        let crop = self.state.crop;
        let cropped_render = self.size_mode() == state::SizeMode::LongestSide && crop.cropped();
        let key = (image.id, self.revision, format!("{crop:?}{orientation:?}"));
        if self.compare.shown_key.as_ref() == Some(&key) {
            return self.compare.shown.clone();
        }
        let pixels = (!cropped_render && crop.cropped()).then(|| crop.pixels(Size::new(image.width as f64, image.height as f64)));
        let shown = image.with_pixels(|rgba| Shown::of(rgba, image.width, image.height, pixels, orientation, image.wide)).flatten();
        if shown.is_some() {
            self.compare.shown_key = Some(key);
            self.compare.shown = shown;
        }
        self.compare.shown.clone()
    }

    /// The original as shown beside the open photograph.
    pub fn compared_original(&self) -> Option<Shown> {
        let (_, picture) = self.original_cache.as_ref()?;
        let crop = self.state.crop;
        let pixels = crop.cropped().then(|| crop.pixels(Size::new(picture.width as f64, picture.height as f64)));
        picture.with_pixels(|rgba| Shown::of(rgba, picture.width, picture.height, pixels, self.state.orientation(), picture.wide)).flatten()
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

    /// "Portra 400 on Endura": a photograph's film and paper, from its sidecar
    /// unless it's open.
    pub fn compared_stocks(&self, item: &Item) -> String {
        let name = |id: &str, paper: bool| self.stocks.iter().find(|s| s.paper == paper && s.id == id).map_or_else(|| id.to_string(), |s| s.name.clone());
        let (film, paper, scanning) = if self.library_item().as_ref() == Some(item) {
            (self.state.film.clone(), self.state.paper.clone(), json::boolean(self.value("io.scan_film")))
        } else {
            let Some(library) = &self.shelf.library else { return String::new() };
            let (stored, _) = library.thumbnail_state(item).unwrap_or_else(|| (library.unedited_state(), false));
            let Ok(state) = library_map::from_library(&stored, false) else { return String::new() };
            let scanning = json::boolean(state.value("io.scan_film"));
            (state.film, state.paper, scanning)
        };
        if scanning { name(&film, false) } else { format!("{} on {}", name(&film, false), name(&paper, true)) }
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

    /// The status line's words in compare: the two, by number and stock.
    pub fn compare_summary(&self) -> String {
        let Some(c) = self.comparison() else { return String::new() };
        let name = |side: &Side| match side {
            Side::Original(_) => self.original_word().to_string(),
            Side::Photo(item) => self.frame_number(item),
        };
        format!("Comparing {} and {}", name(&c.first), name(&c.second))
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
