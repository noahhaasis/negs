//! Compare's view (ticket 46, `CompareCanvas.swift`; ported from
//! `windows/src/ui/compare.rs`): it takes the canvas's place while
//! comparing. Side by side, each photograph in its own frame: over it its
//! number in a ring (the edited one's in pencil) and what the camera wrote
//! down, the facts that differ underlined; under it its film, printed on its
//! paper. As a slider, one frame with the first left of a line that drags,
//! the first's words on the left and the second's on the right. A click on
//! the other photograph edits it. The tools (Side by side · Slider, Swap,
//! Fit · 100%, Done) are in the status line; keys S, ← →, Esc, C, ⌘/Ctrl 0
//! and ⌘/Ctrl 1.
//!
//! At 100% both are linked: the open photograph one pixel to a screen
//! pixel, the other as much past its fit, both about the same point. A drag
//! pans both; in the slider the knob moves the line. A double click zooms
//! between Fit and 100%.
//!
//! Each side's picture is made off the UI thread, colour-managed for the
//! display: at its frame's size for Fit (resampled once, so it isn't
//! aliased by the GPU's sampling), and whole for 100%. Until a picture of
//! the right size is in, the last one made is drawn stretched.

use std::sync::{Arc, Mutex};

use gpui::{Corners, CursorStyle, RenderImage, Window};
use negs_app::compare::{self as rules, Layout, Side};
use negs_app::crop::{CropSettings, Orientation, Point, Rect};
use negs_app::model::compare::Shown;
use negs_app::model::dirty;

use crate::NegsApp;
use crate::color_display::{DisplayColorProfile, SourceRegion, bgra_to_render_image, extract_region_color_managed_bgra, resample_and_color_manage_bgra};
use crate::marks_gpui::paint_ring;
use crate::print::{self, Painter, Target, TextStyle};
use crate::sheet::SheetInk;
use crate::theme_gpui::{hex, hex_alpha};

const NUMBER: TextStyle = TextStyle::serif(15.0, 600.0);
const FACT: TextStyle = TextStyle::serif(15.0, 400.0).italic();
const FILM: TextStyle = TextStyle::serif(24.0, 600.0);
const KIND: TextStyle = TextStyle::serif(14.0, 400.0).italic();
const PAPER: TextStyle = TextStyle::serif(17.5, 400.0);
/// Stacked, the film and paper in the caption (`CompareCaption.stocks`).
const STACKED_FILM: TextStyle = TextStyle::serif(16.0, 600.0);
const STACKED_KIND: TextStyle = TextStyle::serif(13.0, 400.0).italic();
const STACKED_PAPER: TextStyle = TextStyle::serif(15.0, 400.0);
/// Loading original's tag (`DevelopTag`).
const TAG: TextStyle = TextStyle::serif(17.0, 400.0).italic();
/// The number's ring, in DIPs.
const RING: f64 = 26.0;

/// What's written round one photograph.
#[derive(Clone, Default, PartialEq)]
pub struct Words {
    pub number: String,
    /// The camera's facts, once read.
    pub facts: Option<negs_app::photo_facts::PhotoFacts>,
    pub film: String,
    pub paper: Option<String>,
    /// The open photograph's: its names open the tray, as the print label's.
    pub opens: Option<Opens>,
}

/// What the open photograph's label opens, and how it reads (`PrintLabel`).
#[derive(Clone, Default, PartialEq)]
pub struct Opens {
    pub keep_paper: bool,
    /// Read as a negative: "Your negative ⌄ read as <film> ⌄".
    pub read_as: Option<String>,
    pub tray: Option<bool>,
    pub read_as_open: bool,
}

/// A side's picture, colour-managed: at a frame's size, or whole.
struct Picture {
    id: u64,
    /// Physical pixels: the frame's for a fitted one, the picture's own whole.
    size: (u32, u32),
    image: Arc<RenderImage>,
}

/// A picture made on a thread, waiting for the UI thread.
struct Made {
    side: usize,
    id: u64,
    size: (u32, u32),
    whole: bool,
    bgra: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Moves the slider's line.
    Split,
    Pan,
}

#[derive(Clone, Copy, Debug)]
struct Press {
    at: (f64, f64),
    drag: Drag,
    moved: bool,
    centre: (f64, f64),
}

#[derive(Default)]
pub struct CompareView {
    /// What each side shows now, and its pictures (fitted, whole).
    shown: [Option<Shown>; 2],
    fitted: [Option<Picture>; 2],
    whole: [Option<Picture>; 2],
    /// What's being made, by side: (id, size, whole).
    making: [Vec<(u64, (u32, u32), bool)>; 2],
    made: Arc<Mutex<Vec<Made>>>,
    pub words: [Words; 2],
    aspects: [f64; 2],
    /// Where the photographs were last drawn, in window DIPs.
    pub frames: [Rect; 2],
    /// The open photograph's names and lock, in window DIPs.
    pub targets: Vec<(Target, Rect)>,
    pub hot: Option<Target>,
    /// Side by side, one over the other.
    pub column: bool,
    /// At 100% (`model.fit_image` off), the zoom past each fit, and the
    /// point both are centred on, as fractions of the photographs.
    actual: bool,
    relative: f64,
    centre: (f64, f64),
    zoom_command: u64,
    /// How far a DIP moves the centre across the open photograph.
    pan_unit: (f64, f64),
    press: Option<Press>,
    /// The original's side while its picture is on its way, and its tag's words.
    loading: Option<(usize, &'static str)>,
    running: Option<std::time::Instant>,
    /// The side the open photograph is on, which 100% is a pixel to a pixel.
    open: usize,
    pub drops: Vec<Arc<RenderImage>>,
}

impl CompareView {
    fn drop_picture(&mut self, picture: Option<Picture>) {
        if let Some(p) = picture {
            self.drops.push(p.image);
        }
    }

    /// Out of compare: every picture goes.
    pub fn clear(&mut self) {
        for i in 0..2 {
            let (f, w) = (self.fitted[i].take(), self.whole[i].take());
            self.drop_picture(f);
            self.drop_picture(w);
            self.making[i].clear();
        }
        self.shown = [None, None];
        self.press = None;
        self.loading = None;
        self.targets.clear();
    }

    /// Makes side `i`'s picture of `shown` off the UI thread: at `size`, or whole.
    fn request(&mut self, i: usize, shown: &Shown, size: (u32, u32), whole: bool, profile: &DisplayColorProfile) {
        if self.making[i].contains(&(shown.id, size, whole)) {
            return;
        }
        self.making[i].push((shown.id, size, whole));
        let (shown, made, cm) = (shown.clone(), self.made.clone(), profile.transform.clone());
        std::thread::Builder::new()
            .name("compare-picture".into())
            .spawn(move || {
                let bgra = if whole {
                    let region = SourceRegion { x: 0, y: 0, width: shown.width, height: shown.height };
                    extract_region_color_managed_bgra(&shown.rgba, shown.width, shown.height, shown.wide, Orientation::default(), &CropSettings::default(), region, cm.as_deref())
                } else {
                    resample_and_color_manage_bgra(&shown.rgba, shown.width, shown.height, shown.wide, Orientation::default(), &CropSettings::default(), size.0, size.1, cm.as_deref())
                };
                made.lock().unwrap().push(Made { side: i, id: shown.id, size, whole, bgra });
                negs_app::host::post(|m| m.dirty |= dirty::COMPARE);
            })
            .ok();
    }

    /// Pictures made since the last frame, into textures if still wanted.
    fn take_made(&mut self) {
        let made: Vec<Made> = std::mem::take(&mut *self.made.lock().unwrap());
        for m in made {
            self.making[m.side].retain(|k| *k != (m.id, m.size, m.whole));
            if self.shown[m.side].as_ref().map(|s| s.id) != Some(m.id) {
                continue;
            }
            let picture = Picture { id: m.id, size: m.size, image: bgra_to_render_image(m.bgra, m.size.0, m.size.1) };
            let old = if m.whole { self.whole[m.side].replace(picture) } else { self.fitted[m.side].replace(picture) };
            self.drop_picture(old);
        }
    }
}

/// The frame's part of a picture `image` (its pixels) shown at `relative`
/// past its fit, centred on `centre` where the photograph allows: (x, y,
/// width, height) in its pixels. `frame` in physical pixels.
pub fn shown_part(frame: (f64, f64), image: (f64, f64), relative: f64, centre: (f64, f64)) -> (f64, f64, f64, f64) {
    let (fw, fh) = frame;
    let (iw, ih) = image;
    let scale = (fw / iw).max(fh / ih) * relative.max(1.0);
    let (sw, sh) = ((fw / scale).min(iw), (fh / scale).min(ih));
    let sx = (centre.0 * iw - sw / 2.0).clamp(0.0, iw - sw);
    let sy = (centre.1 * ih - sh / 2.0).clamp(0.0, ih - sh);
    (sx, sy, sw, sh)
}

/// `image` (w × h) fitted into `frame`, centred.
pub fn fitted(frame: Rect, image: (f64, f64)) -> Rect {
    let scale = (frame.width / image.0.max(1.0)).min(frame.height / image.1.max(1.0));
    let (w, h) = (image.0 * scale, image.1 * scale);
    Rect::new(frame.x + (frame.width - w) / 2.0, frame.y + (frame.height - h) / 2.0, w, h)
}

/// The frames in window DIPs: the slider's one (twice) or the two, and
/// whether they're one over the other.
pub fn frames(canvas: Rect, layout: Layout, aspects: [f64; 2]) -> ([Rect; 2], bool) {
    let to = |f: rules::Frame| Rect::new(canvas.x + f[0], canvas.y + f[1], f[2], f[3]);
    if layout == Layout::Slider {
        let f = to(rules::slider_frame((canvas.width, canvas.height), aspects[0]));
        ([f, f], false)
    } else {
        let (arrangement, f) = rules::layout((canvas.width, canvas.height), (aspects[0], aspects[1]));
        ([to(f[0]), to(f[1])], arrangement == rules::Arrangement::Column)
    }
}

/// The open photograph's 100% zoom past its fit in a `frame` of physical
/// pixels, and the centre clamped to what can be seen, and how far one
/// physical pixel moves the centre.
pub fn actual(frame: (f64, f64), image: (f64, f64), centre: (f64, f64)) -> (f64, (f64, f64), (f64, f64)) {
    let (fw, fh) = (frame.0.max(1.0), frame.1.max(1.0));
    let (iw, ih) = image;
    let relative = (1.0 / (fw / iw).max(fh / ih)).max(1.0);
    let (_, _, sw, sh) = shown_part((fw, fh), image, relative, centre);
    let (hx, hy) = (sw / iw / 2.0, sh / ih / 2.0);
    let centre = (centre.0.clamp(hx, 1.0 - hx), centre.1.clamp(hy, 1.0 - hy));
    (relative, centre, (sw / iw / fw, sh / ih / fh))
}

/// The knob on the slider's line: within 16 DIPs of its middle.
pub fn on_knob(frame: Rect, split: f64, p: Point) -> bool {
    let line = frame.x + frame.width * split;
    (p.x - line).abs() <= 16.0 && (p.y - frame.mid_y()).abs() <= 16.0
}

/// Brings the pictures and words up to date with the model; called each
/// frame while comparing, before painting.
pub fn sync(app: &mut NegsApp) {
    let Some(comparison) = app.model.comparison().cloned() else {
        if app.compare.shown.iter().any(Option::is_some) {
            app.compare.clear();
        }
        return;
    };
    app.compare.take_made();
    let mut aspects = [1.5; 2];
    for (i, side) in [&comparison.first, &comparison.second].into_iter().enumerate() {
        let shown = app.model.compared_side(side);
        if let Some(s) = &shown {
            aspects[i] = s.aspect();
        }
        app.compare.shown[i] = shown;
        let item = side.item().clone();
        app.compare.words[i] = match side {
            Side::Original(_) => Words { number: String::new(), facts: app.facts.get(&item.photo), film: app.model.original_word().to_string(), paper: None, opens: None },
            Side::Photo(_) => {
                let (mut film, paper) = app.model.compared_stocks(&item);
                let model = &app.model;
                let opens = (model.library_item().as_ref() == Some(&item)).then(|| {
                    let read_as = model.tray().is_some_and(|t| t.read_as);
                    Opens {
                        keep_paper: model.prefs.keep_paper,
                        read_as: model.is_negative().then(|| std::mem::replace(&mut film, negs_app::negative::NAME.to_string())),
                        tray: model.tray_shown().filter(|_| !read_as),
                        read_as_open: model.tray_shown() == Some(false) && read_as,
                    }
                });
                Words { number: app.model.frame_number(&item), facts: app.facts.get(&item.photo), film, paper, opens }
            }
        };
    }
    // The original's side, until its picture is in: the photograph's shape,
    // and Loading original over the slider.
    let mut loading = None;
    if let Some(i) = [&comparison.first, &comparison.second].iter().position(|side| side.is_original()) {
        let id = app.compare.shown[i].as_ref().map(|s| s.id);
        let drawn = app.compare.fitted[i].as_ref().is_some_and(|p| Some(p.id) == id) || app.compare.whole[i].as_ref().is_some_and(|p| Some(p.id) == id);
        if !drawn {
            aspects[i] = aspects[1 - i];
            if comparison.layout == Layout::Slider {
                loading = app.model.compared_original_loading().map(|title| (i, title));
            }
        }
    }
    let view = &mut app.compare;
    if view.loading.is_none() {
        view.running = None;
    }
    view.loading = loading;
    view.aspects = aspects;
    let open = |side: &Side| matches!(side, Side::Photo(item) if app.model.library_item().as_ref() == Some(item));
    view.open = if open(&comparison.first) || !open(&comparison.second) { 0 } else { 1 };
    // Fit and 100% (⌘0, ⌘1, the status line's words) zoom both.
    if view.zoom_command != app.model.zoom_command {
        view.zoom_command = app.model.zoom_command;
        view.actual = !app.model.fit_image;
        view.centre = (0.5, 0.5);
    }
}

/// What the two differ in, for the status line ("shutter and aperture"):
/// nothing against the original, or while either's facts are unknown.
pub fn differences(app: &NegsApp) -> String {
    let Some(c) = app.model.comparison() else { return String::new() };
    let (Side::Photo(a), Side::Photo(b)) = (&c.first, &c.second) else { return String::new() };
    let (fa, fb) = (app.facts.get(&a.photo), app.facts.get(&b.photo));
    let (sa, sb) = (app.model.compared_stocks(a), app.model.compared_stocks(b));
    let (pa, pb) = (fa.map(|f| f.parts('.')), fb.map(|f| f.parts('.')));
    rules::differences(pa.as_ref(), pb.as_ref(), (&sa.0, &sb.0), (sa.1.as_deref(), sb.1.as_deref()))
}

/// The panel's note: "Editing ③ the left one", or the original's.
pub fn note_words(app: &NegsApp) -> Option<(bool, String, String)> {
    let c = app.model.comparison()?;
    let number = |side: &Side| app.model.frame_number(side.item());
    let place = if c.layout == Layout::SideBySide && app.compare.column {
        if c.active_first { "top" } else { "bottom" }
    } else if c.active_first {
        "left"
    } else {
        "right"
    };
    Some(if c.active().is_original() { (true, number(c.inactive()), String::new()) } else { (false, number(c.active()), format!("the {place} one")) })
}

// MARK: Painting

/// Paints compare in `canvas` (window DIPs) on the sheet.
pub fn paint(app_entity: &gpui::Entity<NegsApp>, p: &mut Painter, canvas: Rect, ink: &SheetInk, highlighter_tokens: &negs_app::theme::Tokens) {
    let sf = p.window.scale_factor().max(1.0) as f64;
    let (comparison, profile) = {
        let app = app_entity.read(p.cx);
        (app.model.comparison().cloned(), app.canvas.display_profile.clone())
    };
    let Some(comparison) = comparison else { return };
    let slider = comparison.layout == Layout::Slider;
    let blank = hex(highlighter_tokens.film.blank);
    // Everything the paint needs from the view, worked out first.
    let (frames, words, relative, _centre, draws, loading, phase, column_changed) = app_entity.update(p.cx, |app, _| {
        let v = &mut app.compare;
        let (frames, column) = frames(canvas, comparison.layout, v.aspects);
        let column_changed = v.column != column;
        v.column = column;
        v.frames = frames;
        // At 100%: the open photograph, a pixel to a pixel.
        let open = if v.shown[v.open].is_some() { v.open } else { 1 - v.open };
        let (relative, centre) = match (&v.shown[open], v.actual) {
            (Some(s), true) => {
                let f = frames[open];
                let (relative, centre, unit) = actual((f.width * sf, f.height * sf), (s.width as f64, s.height as f64), v.centre);
                v.centre = centre;
                v.pan_unit = (unit.0 * sf, unit.1 * sf);
                (relative, centre)
            }
            _ => (1.0, (0.5, 0.5)),
        };
        v.relative = relative;
        // Each side: where its picture goes, and which picture.
        let mut draws: [Option<(Rect, Arc<RenderImage>)>; 2] = [None, None];
        for i in 0..2 {
            let Some(shown) = v.shown[i].clone() else { continue };
            let image = (shown.width as f64, shown.height as f64);
            let frame = frames[i];
            if relative > 1.0 {
                // Whole, placed so its shown part fills the frame.
                let (sx, sy, sw, _) = shown_part((frame.width * sf, frame.height * sf), image, relative, centre);
                let scale = frame.width / sw;
                let at = Rect::new(frame.x - sx * scale, frame.y - sy * scale, image.0 * scale, image.1 * scale);
                if v.whole[i].as_ref().is_none_or(|w| w.id != shown.id) {
                    v.request(i, &shown, (shown.width, shown.height), true, &profile);
                }
                let picture = v.whole[i].as_ref().or(v.fitted[i].as_ref());
                draws[i] = picture.map(|p| (at, p.image.clone()));
            } else {
                // The second on the slider fits into the first's frame.
                let at = if slider && i == 1 { fitted(frame, image) } else { frame };
                let size = (((at.width * sf).round() as u32).max(1), ((at.height * sf).round() as u32).max(1));
                if v.fitted[i].as_ref().is_none_or(|f| f.id != shown.id || f.size != size) {
                    v.request(i, &shown, size, false, &profile);
                }
                let picture = v.fitted[i].as_ref().filter(|f| f.id == shown.id).or(v.whole[i].as_ref().filter(|w| w.id == shown.id)).or(v.fitted[i].as_ref());
                draws[i] = picture.map(|p| (at, p.image.clone()));
            }
        }
        let phase = v.loading.map(|_| (v.running.get_or_insert_with(std::time::Instant::now).elapsed().as_secs_f64() / 1.1).fract());
        (frames, v.words.clone(), relative, centre, draws, v.loading, phase, column_changed)
    });
    if column_changed {
        // The note and the status line say top and bottom.
        app_entity.update(p.cx, |app, _| app.model.dirty |= dirty::COMPARE | dirty::STATUS);
    }
    let window = &mut *p.window;
    let paint_in = |window: &mut Window, clip: Rect, draw: &Option<(Rect, Arc<RenderImage>)>| {
        if let Some((at, image)) = draw {
            window.with_content_mask(Some(gpui::ContentMask { bounds: print::bounds(clip) }), |window| {
                let _ = window.paint_image(print::bounds(*at), Corners::default(), image.clone(), 0, false);
            });
        }
    };
    if slider {
        let f = frames[0];
        let split = f.x + f.width * comparison.split;
        window.paint_quad(gpui::fill(print::bounds(f), blank));
        paint_in(window, Rect::new(f.x, f.y, split - f.x, f.height), &draws[0]);
        if relative <= 1.0 {
            // Black where the second's proportions leave room.
            window.paint_quad(gpui::fill(print::bounds(Rect::new(split, f.y, f.max_x() - split, f.height)), gpui::black()));
        }
        paint_in(window, Rect::new(split, f.y, f.max_x() - split, f.height), &draws[1]);
    } else {
        for i in 0..2 {
            window.paint_quad(gpui::fill(print::bounds(frames[i]), blank));
            paint_in(window, frames[i], &draws[i]);
        }
    }
    let edited = |i: usize| comparison.active_first == (i == 0) && !(if i == 0 { &comparison.first } else { &comparison.second }).is_original();
    let hot = app_entity.read(p.cx).compare.hot;
    let mut targets = vec![];
    let w = canvas.max_x();
    if slider {
        let f = frames[0];
        let middle = f.y - 20.0;
        caption(p, ink, &words[0], &words[1], f.x, middle, false, edited(0), false, f.width / 2.0 - 12.0);
        caption(p, ink, &words[1], &words[0], f.max_x(), middle, true, edited(1), false, f.width / 2.0 - 12.0);
        // The first's paper only when it clears the second's film.
        let second = p.measure(&words[1].film, &FILM).width;
        targets.extend(label(p, ink, &words[0], f.x, f.max_y() + 8.0, false, true, f.max_x() - second - 24.0, hot));
        targets.extend(label(p, ink, &words[1], f.max_x(), f.max_y() + 8.0, true, true, f64::NEG_INFINITY, hot));
        // The line, with a grip in its middle.
        let x = f.x + f.width * comparison.split;
        let cy = f.mid_y();
        let white = gpui::white().opacity(0.92);
        p.fill(Rect::new(x - 0.75, f.y, 1.5, f.height), white);
        p.window.paint_quad(
            gpui::quad(print::bounds(Rect::new(x - 13.0, cy - 13.0, 26.0, 26.0)), Corners::all(gpui::px(13.0)), hex_alpha(0x111111, 0.85), gpui::Edges::all(gpui::px(1.5)), white, gpui::BorderStyle::Solid),
        );
        for dir in [-1.0, 1.0] {
            let mut b = gpui::PathBuilder::stroke(gpui::px(1.4));
            let at = |dx: f64, dy: f64| gpui::point(gpui::px((x + dir * dx) as f32), gpui::px((cy + dy) as f32));
            b.move_to(at(3.0, -4.0));
            b.line_to(at(6.5, 0.0));
            b.line_to(at(3.0, 4.0));
            if let Ok(path) = b.build() {
                p.window.paint_path(path, white);
            }
        }
    } else if app_entity.read(p.cx).compare.column {
        // Stacked: one line over each, the film and paper on its right.
        for i in 0..2 {
            let f = frames[i];
            let middle = f.y - rules::STACKED_CAPTION / 2.0;
            let ring = if words[i].number.is_empty() { 0.0 } else { RING + 12.0 };
            let width = stocks(p, ink, &words[i], &words[1 - i], f.max_x(), middle, f.x + ring);
            caption(p, ink, &words[i], &words[1 - i], f.x, middle, false, edited(i), true, f.width - width - 24.0);
        }
    } else {
        for i in 0..2 {
            let f = frames[i];
            // As wide as its frame, or the room up to the other's.
            let room = if i == 0 { (frames[1].x - f.x - 24.0).max(f.width) } else { w - f.x - 16.0 };
            caption(p, ink, &words[i], &words[1 - i], f.x, f.y - 20.0, false, edited(i), true, room);
            targets.extend(label(p, ink, &words[i], f.x, f.max_y() + 8.0, false, false, w - 12.0, hot));
        }
    }
    // Loading original over the slider.
    if let (Some((_, title)), Some(phase)) = (loading, phase) {
        loading_tag(p, frames[0], title, phase, highlighter_tokens);
        p.window.request_animation_frame();
    }
    app_entity.update(p.cx, |app, _| app.compare.targets = targets);
}

/// The caption's facts; the slider's leave out the camera.
fn items(words: &Words, camera: bool) -> Vec<String> {
    let Some(facts) = &words.facts else { return vec![] };
    let facts = if camera { facts.clone() } else { negs_app::photo_facts::PhotoFacts { camera: None, ..facts.clone() } };
    facts.items(negs_app::host::decimal_separator())
}

/// Words centred on `middle` from `x`; returns their metrics.
fn centred(p: &mut Painter, text: &str, style: TextStyle, color: gpui::Hsla, x: f64, middle: f64) -> print::Metrics {
    let m = p.measure(text, &style);
    p.word(text, style, color, x, middle - m.height / 2.0 + m.baseline)
}

/// Over a photograph: its number in a ring, then its facts, those the
/// other's differ in underlined. `right` lays it out leftwards from `edge`.
#[allow(clippy::too_many_arguments)]
fn caption(p: &mut Painter, ink: &SheetInk, words: &Words, other: &Words, edge: f64, middle: f64, right: bool, edited: bool, camera: bool, room: f64) {
    const DOT_GAP: f64 = 9.0;
    let dot_width = p.measure("·", &FACT).width;
    let has_number = !words.number.is_empty();
    let number_width = if has_number { RING + 12.0 } else { 0.0 };
    let mut camera = camera;
    let (mut facts, mut others) = (items(words, camera), items(other, camera));
    let mut widths: Vec<f64>;
    let mut facts_width;
    loop {
        widths = facts.iter().map(|f| p.measure(f, &FACT).width).collect();
        facts_width = widths.iter().sum::<f64>() + facts.len().saturating_sub(1) as f64 * (dot_width + 2.0 * DOT_GAP);
        if number_width + facts_width <= room || facts.is_empty() {
            break;
        }
        if camera && words.facts.as_ref().is_some_and(|f| f.camera.is_some()) {
            camera = false;
            (facts, others) = (items(words, false), items(other, false));
        } else {
            facts.pop();
        }
    }
    let total = number_width + facts_width;
    let x0 = if right { edge - total } else { edge };
    if has_number {
        let rx = if right { edge - RING } else { x0 };
        let r = Rect::new(rx, middle - RING / 2.0, RING, RING);
        if edited {
            paint_ring(p.window, print::bounds(r), negs_marks::Radii::NUMBER, -6.0, p.plain, hex(ink.accent));
        } else {
            p.window.paint_quad(gpui::quad(
                print::bounds(r.inset(1.0, 1.0)),
                Corners::all(gpui::px((RING / 2.0 - 1.0) as f32)),
                gpui::transparent_black(),
                gpui::Edges::all(gpui::px(1.2)),
                hex(ink.dim),
                gpui::BorderStyle::Solid,
            ));
        }
        let m = p.measure(&words.number, &NUMBER);
        centred(p, &words.number, NUMBER, hex(ink.paper), rx + (RING - m.width) / 2.0, middle);
    }
    let mut x = if right { x0 } else { x0 + number_width };
    for (i, (fact, w)) in facts.iter().zip(&widths).enumerate() {
        if i > 0 {
            centred(p, "·", FACT, hex(ink.dim), x + DOT_GAP, middle);
            x += dot_width + 2.0 * DOT_GAP;
        }
        let m = centred(p, fact, FACT, hex(ink.label), x, middle);
        let baseline = middle - m.height / 2.0 + m.baseline;
        // Only what differs, and only once the other's facts are known.
        let differs = if i > 0 || !camera { !others.is_empty() && !others.contains(fact) } else { !others.is_empty() && others.first() != Some(fact) };
        if differs {
            print::open_mark(p, x, baseline + 3.0, *w, ink);
        }
        x += w;
    }
}

/// Stacked, on the caption's right ending at `edge`: "Kodak Gold 200 on
/// Portra Endura", each name marked when it differs from the other's.
/// Never left of `start`. Returns its width.
#[allow(clippy::too_many_arguments)]
fn stocks(p: &mut Painter, ink: &SheetInk, words: &Words, other: &Words, edge: f64, middle: f64, start: f64) -> f64 {
    const GAP: f64 = 6.0;
    if words.number.is_empty() {
        let m = p.measure(&words.film, &FACT);
        centred(p, &words.film, FACT, hex(ink.dim), (edge - m.width).max(start), middle);
        return m.width;
    }
    let compared = !other.number.is_empty();
    let film_marked = compared && words.film != other.film;
    let paper_marked = compared && words.paper.is_some() && other.paper.is_some() && words.paper != other.paper;
    let name_ink = |marked: bool| hex(if words.opens.is_some() || marked { ink.paper } else { ink.label });
    let kind = if words.paper.is_some() { "on" } else { "scanned" };
    let fm = p.measure(&words.film, &STACKED_FILM);
    let km = p.measure(kind, &STACKED_KIND);
    let pm = words.paper.as_ref().map(|paper| p.measure(paper, &STACKED_PAPER));
    let width = fm.width + GAP + km.width + pm.map_or(0.0, |m| GAP + m.width);
    let baseline = middle - fm.height / 2.0 + fm.baseline;
    let mut x = (edge - width).max(start);
    p.word(&words.film, STACKED_FILM, name_ink(film_marked), x, baseline);
    if film_marked {
        print::open_mark(p, x, baseline + 3.0, fm.width, ink);
    }
    x += fm.width + GAP;
    p.word(kind, STACKED_KIND, hex(ink.dim), x, baseline);
    x += km.width + GAP;
    if let (Some(paper), Some(pm)) = (&words.paper, pm) {
        p.word(paper, STACKED_PAPER, name_ink(paper_marked), x, baseline);
        if paper_marked {
            print::open_mark(p, x, baseline + 3.0, pm.width, ink);
        }
    }
    width
}

/// Under a photograph: its film, and on the next line (or after it,
/// `inline`, when it ends before `limit`) "printed on" its paper. The open
/// photograph's names keep their `⌄` and the paper its lock; returns where
/// they take clicks.
#[allow(clippy::too_many_arguments)]
fn label(p: &mut Painter, ink: &SheetInk, words: &Words, edge: f64, top: f64, right: bool, inline: bool, limit: f64, hot: Option<Target>) -> Vec<(Target, Rect)> {
    use print::{CARET, INNER, LOCK};
    let mut targets = vec![];
    let opens = words.opens.as_ref();
    let caret = if opens.is_some() { INNER + CARET.0 + 1.0 } else { 0.0 };
    let lit = |t: Target, base: u32| hex(if hot == Some(t) { ink.paper } else { base });
    let fm = p.measure(&words.film, &FILM);
    let read_as = opens.and_then(|o| o.read_as.as_deref());
    let (km_read, rm) = (p.measure("read as", &KIND), p.measure(read_as.unwrap_or(""), &PAPER));
    let read_width = if read_as.is_some() { INNER + km_read.width + INNER + rm.width + caret } else { 0.0 };
    let fx = if right { edge - fm.width - caret - read_width } else { edge };
    let baseline = top + fm.baseline;
    p.word(&words.film, FILM, hex(ink.paper), fx, baseline);
    if let Some(o) = opens {
        print::paint_caret(p.window, fx + fm.width + INNER + 1.0, baseline - 6.0, lit(Target::Film, ink.dim));
        targets.push((Target::Film, Rect::new(fx, top, fm.width + caret, fm.height + 5.0)));
        if o.tray == Some(false) {
            print::open_mark(p, fx, baseline + 3.0, fm.width, ink);
        }
        if let Some(name) = read_as {
            let x = fx + fm.width + caret + INNER;
            p.word("read as", KIND, hex(ink.dim), x, baseline);
            let x = x + km_read.width + INNER;
            p.word(name, PAPER, hex(ink.paper), x, baseline);
            print::paint_caret(p.window, x + rm.width + INNER + 1.0, baseline - 6.0, lit(Target::ReadAs, ink.dim));
            targets.push((Target::ReadAs, Rect::new(x, baseline - rm.baseline, rm.width + caret, rm.height + 5.0)));
            if o.read_as_open {
                print::open_mark(p, x, baseline + 3.0, rm.width, ink);
            }
        }
    }
    let Some(paper) = words.paper.as_ref() else { return targets };
    let inline = inline && read_as.is_none();
    let kind = if opens.is_some_and(|o| o.keep_paper) { "always printed on" } else { "printed on" };
    let lock = if opens.is_some() { LOCK.0 + 5.0 } else { 0.0 };
    let km = p.measure(kind, &KIND);
    let pm = p.measure(paper, &PAPER);
    let film_end = fx + fm.width + caret;
    let end = if inline { film_end + 14.0 } else { edge } + lock + km.width + 8.0 + pm.width + caret;
    if end > limit {
        return targets;
    }
    let (x, baseline) = if inline { (film_end + 14.0, baseline) } else { (edge, top + fm.height + 4.0 + pm.baseline) };
    let x = if right && !inline { edge - lock - km.width - 8.0 - pm.width - caret } else { x };
    let x = match opens {
        Some(o) => {
            let (lock_ink, kind_ink) = if o.keep_paper { (ink.accent, ink.label) } else { (ink.track, ink.dim) };
            print::paint_lock(p.window, o.keep_paper, x, baseline - LOCK.1 + 1.0, lit(Target::Lock, lock_ink));
            p.word(kind, KIND, hex(kind_ink), x + lock, baseline);
            targets.push((Target::Lock, Rect::new(x, baseline - 14.0, lock + km.width, 19.0)));
            x + lock
        }
        None => {
            p.word(kind, KIND, hex(ink.dim), x, baseline);
            x
        }
    };
    let px_ = x + km.width + 8.0;
    p.word(paper, PAPER, hex(ink.paper), px_, baseline);
    if let Some(o) = opens {
        print::paint_caret(p.window, px_ + pm.width + INNER + 1.0, baseline - 6.0, lit(Target::Paper, ink.dim));
        targets.push((Target::Paper, Rect::new(px_, baseline - pm.baseline, pm.width + caret, pm.height + 5.0)));
        if o.tray == Some(true) {
            print::open_mark(p, px_, baseline + 3.0, pm.width, ink);
        }
    }
    targets
}

/// The develop tag over `frame` (`DevelopTag`): the words over a short line
/// where a yellow mark runs every 1.1 s.
fn loading_tag(p: &mut Painter, frame: Rect, title: &str, phase: f64, t: &negs_app::theme::Tokens) {
    let m = p.measure(title, &TAG);
    let line = 64.0;
    let (pad_x, pad_top, pad_bottom, gap) = (16.0, 7.0, 9.0, 5.0);
    let (w, h) = ((m.width.max(line) + 2.0 * pad_x).round(), (pad_top + m.height + gap + 1.0 + pad_bottom).round());
    let x = (frame.x + (frame.width - w) / 2.0).round().max(0.0);
    let y = (frame.y + 16.0).round();
    let r = Rect::new(x, y, w, h);
    p.window.paint_quad(gpui::quad(print::bounds(r), Corners::all(gpui::px(3.0)), hex_alpha(t.hex.bg, 0.78), gpui::Edges::all(gpui::px(1.0)), hex_alpha(t.hex.paper, 0.08), gpui::BorderStyle::Solid));
    p.word(title, TAG, hex(t.hex.paper), x + (w - m.width) / 2.0, y + pad_top - 1.0 + m.baseline);
    let (lx, ly) = ((x + (w - line) / 2.0).round(), (y + pad_top + m.height + gap).round());
    p.fill(Rect::new(lx, ly, line, 1.0), hex(t.hex.track));
    let start = lx + line * (phase * 1.3 - 0.3);
    let (a, b) = (start.max(lx), (start + line * 0.3).min(lx + line));
    if b > a {
        p.fill(Rect::new(a, ly, b - a, 1.0), hex(t.hex.accent));
    }
}

// MARK: Pointer

pub fn cursor(app: &NegsApp, p: Point) -> CursorStyle {
    let v = &app.compare;
    let Some(c) = app.model.comparison() else { return CursorStyle::Arrow };
    if print::hit(&v.targets, p).is_some() {
        return CursorStyle::PointingHand;
    }
    let over = v.frames.iter().any(|f| f.contains(p));
    let slider = c.layout == Layout::Slider && (v.relative <= 1.0 || on_knob(v.frames[0], c.split, p));
    if over && !slider {
        if v.relative > 1.0 { CursorStyle::OpenHand } else { CursorStyle::PointingHand }
    } else if over {
        CursorStyle::ResizeLeftRight
    } else {
        CursorStyle::Arrow
    }
}

/// A press while comparing. Returns a label target pressed, for the canvas
/// to act on when the button comes up.
pub fn mouse_down(app: &mut NegsApp, p: Point, clicks: usize) -> Option<Target> {
    if let Some(target) = print::hit(&app.compare.targets, p) {
        return Some(target);
    }
    let comparison = app.model.comparison()?.clone();
    let v = &mut app.compare;
    let i = v.frames.iter().position(|f| f.contains(p))?;
    if clicks >= 2 {
        // Fit ↔ 100%, both photographs.
        let fit = !app.model.fit_image;
        app.model.zoom(fit);
        v.press = None;
        return None;
    }
    let slider = comparison.layout == Layout::Slider;
    let actual = v.relative > 1.0;
    if !slider && !actual {
        // Fitted side by side: a press edits it.
        app.model.activate_compared(i == 0);
        return None;
    }
    let f = v.frames[0];
    let knob = on_knob(f, comparison.split, p);
    let drag = if slider && (!actual || knob) { Drag::Split } else { Drag::Pan };
    v.press = Some(Press { at: (p.x, p.y), drag, moved: false, centre: v.centre });
    if drag == Drag::Split && knob {
        app.model.move_split((p.x - f.x) / f.width.max(1.0));
    }
    None
}

/// The pointer moves with the button down. True if compare used it.
pub fn mouse_drag(app: &mut NegsApp, p: Point) -> bool {
    let Some(mut press) = app.compare.press else { return false };
    let (dx, dy) = (p.x - press.at.0, p.y - press.at.1);
    if !press.moved && dx.abs() + dy.abs() <= 2.0 {
        return true;
    }
    press.moved = true;
    app.compare.press = Some(press);
    match press.drag {
        Drag::Split => {
            let f = app.compare.frames[0];
            app.model.move_split((p.x - f.x) / f.width.max(1.0));
        }
        Drag::Pan => {
            let v = &mut app.compare;
            v.centre = (press.centre.0 - dx * v.pan_unit.0, press.centre.1 - dy * v.pan_unit.1);
        }
    }
    true
}

/// The button came up: a click without a drag edits the photograph under
/// it, or on its side of the line. True if compare had the press.
pub fn mouse_up(app: &mut NegsApp, p: Point) -> bool {
    let Some(press) = app.compare.press.take() else { return false };
    if !press.moved
        && let Some(c) = app.model.comparison()
    {
        let frames = app.compare.frames;
        let first = if c.layout == Layout::Slider { (p.x - frames[0].x) / frames[0].width.max(1.0) < c.split } else { !frames[1].contains(p) };
        app.model.activate_compared(first);
    }
    true
}

/// Keys while comparing: Esc, ← →, S. True if used.
pub fn key(app: &mut NegsApp, key: &str) -> bool {
    if app.model.comparison().is_none() {
        return false;
    }
    match key {
        "escape" => leave(app),
        "left" => app.model.step_compared(-1),
        "right" => app.model.step_compared(1),
        "s" => app.model.toggle_compare_layout(),
        _ => return false,
    }
    true
}

/// Leaves compare for where it was entered.
pub fn leave(app: &mut NegsApp) {
    let origin = app.model.comparison().map(|c| c.origin);
    app.model.leave_compare();
    if origin == Some(rules::Origin::Grid) {
        crate::grid::open(app);
    }
}

/// C and View › Compare: in, from the grid's or strip's selection, or out.
pub fn toggle(app: &mut NegsApp) {
    if app.model.comparison().is_some() {
        return leave(app);
    }
    let in_grid = app.grid.shown;
    let origin = if in_grid { rules::Origin::Grid } else { rules::Origin::Photo };
    app.model.toggle_compare(origin);
    if in_grid && app.model.comparison().is_some() {
        crate::grid::close_for_compare(app);
    }
}

/// The context menu's Compare chosen.
pub fn take_offer(app: &mut NegsApp) {
    let in_grid = app.grid.shown;
    if app.model.take_compare_offer(if in_grid { rules::Origin::Grid } else { rules::Origin::Photo }) && in_grid {
        crate::grid::close_for_compare(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_one_hundred_percent_the_open_photograph_is_a_pixel_to_a_pixel() {
        // A 3000 × 2000 picture in a 600 × 400 frame: five times its fit.
        let (relative, centre, unit) = actual((600.0, 400.0), (3000.0, 2000.0), (0.0, 1.0));
        assert!((relative - 5.0).abs() < 1e-9);
        // The centre can't go past what shows: a fifth of the picture.
        assert!((centre.0 - 0.1).abs() < 1e-9 && (centre.1 - 0.9).abs() < 1e-9);
        assert!((unit.0 - 1.0 / 3000.0).abs() < 1e-12);
        let (sx, sy, sw, sh) = shown_part((600.0, 400.0), (3000.0, 2000.0), relative, (0.5, 0.5));
        assert_eq!((sx, sy, sw, sh), (1200.0, 800.0, 600.0, 400.0));
        // A small picture is never enlarged past its fit.
        assert_eq!(actual((600.0, 400.0), (300.0, 200.0), (0.5, 0.5)).0, 1.0);
    }

    #[test]
    fn the_slider_has_one_frame_and_side_by_side_two() {
        let canvas = Rect::new(100.0, 40.0, 1200.0, 700.0);
        let (f, column) = frames(canvas, Layout::Slider, [1.5, 0.667]);
        assert_eq!(f[0], f[1]);
        assert!(!column);
        assert!((f[0].width / f[0].height - 1.5).abs() < 0.01);
        assert!(f[0].x >= canvas.x + rules::SIDE - 0.5);
        let (f, column) = frames(canvas, Layout::SideBySide, [1.5, 1.5]);
        assert!(!column);
        assert!(f[1].x > f[0].max_x());
        // Two panoramas stack.
        let (f, column) = frames(canvas, Layout::SideBySide, [3.0, 3.0]);
        assert!(column && f[1].y > f[0].max_y());
    }

    #[test]
    fn the_knob_and_the_second_picture_fitted_into_the_slider() {
        let f = Rect::new(0.0, 0.0, 600.0, 400.0);
        assert!(on_knob(f, 0.5, Point { x: 310.0, y: 205.0 }));
        assert!(!on_knob(f, 0.5, Point { x: 330.0, y: 205.0 }));
        let r = fitted(f, (200.0, 400.0));
        assert_eq!(r, Rect::new(200.0, 0.0, 200.0, 400.0));
    }
}
