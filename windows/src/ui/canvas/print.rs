//! The print on the canvas (IMPLEMENTATION.md §4.1–§4.3, §4.10): the
//! caption over the photograph (`Caption.swift`), the label under it
//! (`PrintLabel.swift`) with the paper lock, the trim marks beside its
//! corners that start cropping (`SmallStates.swift`), and while cropping the
//! straightening dial in the label's place (`Crop.swift`).
//!
//! Everything here is laid out in DIPs and drawn in the sheet's inks
//! (`sheet.rs`): words as DirectWrite masks, marks as Direct2D masks, both
//! coloured on the GPU (or by the software canvas).

use super::*;
use crate::ui::d2d::{self, Ink, Painter, RectF, TextMetrics};
use crate::ui::sheet::SheetInk;
use negs_marks::{Path, Pt, Seg};

/// Space round the print (§2.3): the side margins, the caption's band above
/// and the label's two rows below.
pub(super) const MARGIN: f64 = 48.0;
pub(super) const CAPTION_BAND: f64 = 54.0;
pub(super) const LABEL_BAND: f64 = 68.0;

const CAPTION: TextStyle = TextStyle::serif(15.0, 400).italic();
const CAPTION_UPRIGHT: TextStyle = TextStyle::serif(15.0, 400);
const FILM: TextStyle = TextStyle::serif(24.0, 600).tracking(-0.01);
const KIND: TextStyle = TextStyle::serif(14.0, 400).italic();
const PAPER: TextStyle = TextStyle::serif(17.5, 400);
const THE_ORIGINAL: TextStyle = TextStyle::serif(16.0, 400).italic();
const ANGLE: TextStyle = TextStyle::serif(15.0, 400);

/// Between the film and the paper on one row, and inside a name.
const GAP: f64 = 9.0;
const INNER: f64 = 7.0;
/// The `⌄` after a name, and the lock before "printed on".
const CARET: (f64, f64) = (9.0, 8.0);
const LOCK: (f64, f64) = (10.0, 12.0);
const LOCK_SPACING: f64 = 5.0;

/// The trim marks: 18 × 1.5, reaching 26 outside each corner; they take
/// clicks ±4 across and ±9 along their line.
const MARK_LENGTH: f64 = 18.0;
const MARK_REACH: f64 = 26.0;
const MARK_WEIGHT: f64 = 1.5;

/// The straightening dial: 362 wide, 61 ticks a degree apart.
pub(super) const DIAL_WIDTH: f64 = 362.0;
const DIAL_SPACING: f64 = (DIAL_WIDTH - 2.0) / 60.0;
const DIAL_HEIGHT: f64 = 18.0;

/// What the caption and label say, from the model.
#[derive(Clone, Default, PartialEq)]
pub struct PrintWords {
    /// The camera's facts (`PhotoFacts::items`), and the file's name.
    pub facts: Vec<String>,
    pub file: String,
    pub film: String,
    pub paper: String,
    /// Scanning the film: the paper is struck and doesn't open.
    pub scanning: bool,
    /// Holding Original: both names struck, "the original" after them.
    pub original: bool,
    /// The paper lock is closed: a new film keeps the paper.
    pub keep_paper: bool,
    /// The film's (false) or the paper's (true) tray is open: the pencil under its name.
    pub tray: Option<bool>,
}

/// What a click on the canvas can reach, outside cropping's frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    Film,
    Paper,
    Lock,
    /// A trim mark, by the corner it's beside.
    Mark(Handle),
    TurnLeft,
    TurnRight,
}

/// A mark drawn as a mask: its size in DIPs follows from the kind.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Glyph {
    /// The `⌄` after a stock's name.
    Caret,
    /// The paper lock, closed or open.
    Lock(bool),
    /// A quarter-turn arrow, clockwise or not.
    Turn(bool),
    /// The pencil's strike through a name, this many tenths of a DIP wide.
    Strike(u32),
    /// Light table's highlighter under a name, this many tenths of a DIP wide.
    Highlight(u32),
    /// The film strip's leader, raised under the pointer: its slanted edge
    /// anti-aliased, under the leader's window (`strip::shape_leader`).
    Tongue(bool),
}

impl Glyph {
    pub fn size(self) -> (f64, f64) {
        match self {
            Glyph::Caret => CARET,
            Glyph::Lock(_) => LOCK,
            Glyph::Turn(_) => (24.0, 24.0),
            Glyph::Strike(w) => (w as f64 / 10.0, 6.0),
            Glyph::Highlight(w) => (w as f64 / 10.0, 6.0 + negs_marks::HIGHLIGHTER_WIDTH),
            Glyph::Tongue(_) => (crate::sheet_rules::LEADER_WIDTH, crate::sheet_rules::LEADER_RAISED),
        }
    }

    /// Draws it in white at the origin, for `d2d::shape_mask`.
    pub fn draw(self, p: &mut Painter) {
        let white = Ink::hex(0xffffff);
        match self {
            Glyph::Caret => {
                p.line((1.5, 2.5), (4.5, 5.5), 1.15, white);
                p.line((4.5, 5.5), (7.5, 2.5), 1.15, white);
            }
            Glyph::Lock(closed) => draw_lock(p, closed, 0.0, 0.0, white),
            Glyph::Turn(clockwise) => {
                // `arrow.clockwise` at 14 pt: most of a circle, its head at the top.
                let id = if clockwise { 1 } else { 2 };
                let arrow = move || {
                    let (cx, cy, r) = (12.0, 12.5, 5.6);
                    let mut segs = vec![];
                    // From the top, round the far side to 300°.
                    let steps = 5;
                    let (start, sweep) = (-90.0f64, 300.0f64);
                    let dir = if clockwise { 1.0 } else { -1.0 };
                    let point = |deg: f64| Pt { x: cx + r * deg.to_radians().cos(), y: cy + r * deg.to_radians().sin() };
                    segs.push(Seg::Move(point(start + dir * 20.0)));
                    let step = sweep / steps as f64;
                    for i in 0..steps {
                        let a0 = (start + dir * (20.0 + step * i as f64)).to_radians();
                        let a1 = (start + dir * (20.0 + step * (i + 1) as f64)).to_radians();
                        let k = 4.0 / 3.0 * ((a1 - a0) / 4.0).tan() * r;
                        let p0 = (cx + r * a0.cos(), cy + r * a0.sin());
                        let p1 = (cx + r * a1.cos(), cy + r * a1.sin());
                        segs.push(Seg::Cubic(
                            Pt { x: p0.0 - k * a0.sin(), y: p0.1 + k * a0.cos() },
                            Pt { x: p1.0 + k * a1.sin(), y: p1.1 - k * a1.cos() },
                            Pt { x: p1.0, y: p1.1 },
                        ));
                    }
                    Path(segs)
                };
                p.path(0x7a40 + id, arrow, 0.0, 0.0, Some(1.3), white);
                // The head, at the arc's start, pointing along it.
                let (hx, hy) = (12.0 + 5.6 * (-70.0f64).to_radians().cos() * if clockwise { 1.0 } else { -1.0 }, 12.5 + 5.6 * (-70.0f64).to_radians().sin());
                let head = move || {
                    let s = if clockwise { 1.0 } else { -1.0 };
                    Path::polygon(&[Pt { x: hx - s * 3.6, y: hy - 2.9 }, Pt { x: hx + s * 1.4, y: hy - 0.2 }, Pt { x: hx - s * 2.6, y: hy + 3.0 }])
                };
                p.path(0x7a50 + id, head, 0.0, 0.0, None, white);
            }
            Glyph::Strike(w) => p.pencil(RectF::new(0.0, 0.0, w as f32 / 10.0, 6.0), white),
            Glyph::Highlight(w) => {
                let inset = negs_marks::HIGHLIGHTER_WIDTH as f32 / 2.0;
                p.pencil_width(RectF::new(inset, inset, w as f32 / 10.0 - 2.0 * inset, 6.0), negs_marks::HIGHLIGHTER_WIDTH as f32, white);
            }
            Glyph::Tongue(raised) => {
                use crate::sheet_rules::{LEADER_HEIGHT, LEADER_RAISED, LEADER_SLANT, LEADER_WIDTH};
                // `polygon(0 100%, 28px 0, 100% 0, 100% 100%)`.
                let shape = move || {
                    let (w, h) = (LEADER_WIDTH, LEADER_RAISED);
                    let top = h - if raised { LEADER_RAISED } else { LEADER_HEIGHT };
                    Path::polygon(&[Pt { x: 0.0, y: h }, Pt { x: LEADER_SLANT, y: top }, Pt { x: w, y: top }, Pt { x: w, y: h }])
                };
                p.path(0x7a60 + raised as u64, shape, 0.0, 0.0, None, white);
            }
        }
    }
}

/// The paper lock, `LOCK` DIPs, with its top left at (`x`, `y`): SF
/// Symbols' `lock` and `lock.open` at 10.5 pt medium, a body with a
/// shackle over it, swung up and right when open. Also the paper tray's
/// switch (`ui::tray`).
pub fn draw_lock(p: &mut Painter, closed: bool, x: f32, y: f32, ink: Ink) {
    p.fill_round_rect(RectF::new(x + 1.0, y + 5.5, 8.0, 6.0), 1.4, ink);
    let id = if closed { 1 } else { 2 };
    let shackle = move || {
        let (left, right, top, foot) = if closed { (2.9, 7.1, 1.2, 5.8) } else { (5.4, 9.6, 0.2, 5.8) };
        let r = (right - left) / 2.0;
        let k = 0.5523 * r;
        let left_foot = if closed { foot } else { 3.4 };
        Path(vec![
            Seg::Move(Pt { x: left, y: left_foot }),
            Seg::Line(Pt { x: left, y: top + r }),
            Seg::Cubic(Pt { x: left, y: top + r - k }, Pt { x: left + r - k, y: top }, Pt { x: left + r, y: top }),
            Seg::Cubic(Pt { x: left + r + k, y: top }, Pt { x: right, y: top + r - k }, Pt { x: right, y: top + r }),
            Seg::Line(Pt { x: right, y: foot }),
        ])
    };
    p.path(0x10c0 + id, shackle, x, y, Some(1.3), ink);
}

/// `text` in `style`, measured in DIPs (zero until Direct2D is ready).
fn measure(text: &str, style: &TextStyle) -> TextMetrics {
    d2d::measure_text(text, style, 0.0, super::super::theme::tokens().plain)
}

/// `text` cut with an ellipsis to fit `room` DIPs.
fn fitted(text: &str, style: &TextStyle, room: f64) -> String {
    if measure(text, style).width as f64 <= room {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut low, mut high) = (0, chars.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>().trim_end().to_string() + "…";
        if measure(&candidate, style).width as f64 <= room { low = mid } else { high = mid - 1 }
    }
    chars[..low].iter().collect::<String>().trim_end().to_string() + "…"
}

/// The trim marks beside `print`'s corners, level with its top and bottom
/// edges, with the corner each is beside.
pub(super) fn trim_marks(print: Rect, s: f64) -> [(Rect, Handle); 4] {
    let (length, reach, weight) = (MARK_LENGTH * s, MARK_REACH * s, MARK_WEIGHT * s);
    let (top, bottom) = (print.y, print.max_y() - weight);
    let (left, right) = (print.x - reach, print.max_x() + reach - length);
    [
        (Rect::new(left, top, length, weight), Handle { x: -1, y: -1 }),
        (Rect::new(right, top, length, weight), Handle { x: 1, y: -1 }),
        (Rect::new(left, bottom, length, weight), Handle { x: -1, y: 1 }),
        (Rect::new(right, bottom, length, weight), Handle { x: 1, y: 1 }),
    ]
}

/// The photograph's place while cropping, as the print's: fitted between
/// the caption's and the label's bands and the side margins (`CropEditor.place`).
pub(super) fn crop_place(width: f64, height: f64, s: f64, photo: Size) -> (Rect, f64) {
    let (inset, top, bottom) = (MARGIN * s, CAPTION_BAND * s, LABEL_BAND * s);
    let room = Size::new((width - 2.0 * inset).max(1.0), (height - top - bottom).max(1.0));
    let scale = if photo.width > 0.0 && photo.height > 0.0 { (room.width / photo.width).min(room.height / photo.height) } else { 1.0 };
    let fitted = Size::new(photo.width * scale, photo.height * scale);
    (Rect::new(((width - fitted.width) / 2.0).round(), (top + (room.height - fitted.height) / 2.0).round(), fitted.width, fitted.height), scale)
}

/// The dial's tick row under `frame`, 10 DIPs below it, with its angle
/// over it.
pub(super) fn dial_rect(frame: Rect, s: f64) -> Rect {
    let w = DIAL_WIDTH * s;
    let angle_height = 20.0 * s;
    Rect::new((frame.mid_x() - w / 2.0).round(), (frame.max_y() + 10.0 * s + angle_height + 4.0 * s).round(), w, DIAL_HEIGHT * s)
}

/// The quarter-turn arrows either side of the dial: 24 DIPs square, 34
/// outside its ends, 4 above its ticks.
pub(super) fn turn_rects(dial: Rect, s: f64) -> [(Rect, bool); 2] {
    let size = 24.0 * s;
    [
        (Rect::new(dial.x - 34.0 * s, dial.y - 4.0 * s, size, size), false),
        (Rect::new(dial.max_x() + 34.0 * s - size, dial.y - 4.0 * s, size, size), true),
    ]
}

/// DIPs dragged per degree on the dial.
pub(super) fn dial_spacing(s: f64) -> f64 {
    DIAL_SPACING * s
}

/// "0,0°", in the reader's decimal mark.
fn degrees(angle: f64, decimal: char) -> String {
    let angle = if angle == 0.0 { 0.0 } else { angle };
    format!("{angle:.1}°").replace('.', &decimal.to_string())
}

impl View {
    /// Words in `style` with their baseline at `baseline` and their left at
    /// `x`, DIPs; returns their metrics.
    fn word(&mut self, items: &mut Vec<Draw>, text: &str, style: TextStyle, color: u32, x: f64, baseline: f64) -> TextMetrics {
        let m = measure(text, &style);
        if m.width > 0.0 {
            let s = self.scale();
            items.push(Draw::Words(text.into(), style, 0.0, hex(color, 1.0), ((x * s).round() - 1.0, ((baseline - m.baseline as f64) * s).round() - 1.0)));
        }
        m
    }

    fn glyph(&mut self, items: &mut Vec<Draw>, glyph: Glyph, color: u32, alpha: f32, x: f64, y: f64) {
        let s = self.scale();
        items.push(Draw::Glyph(glyph, hex(color, alpha), ((x * s).round(), (y * s).round())));
    }

    /// The caption over `print` (pixels): the facts in italic `label`, `·`
    /// between them in `track`, the file name at the right in `dim`, its
    /// line's foot 10 DIPs above the print. The file name goes first when
    /// they don't fit; a portrait's caption is centred and has none.
    pub(super) fn build_caption(&mut self, items: &mut Vec<Draw>, print: Rect, portrait: bool, ink: &SheetInk) {
        let s = self.scale();
        let words = self.scene.words.clone();
        let w = self.width as f64 / s;
        let left = (print.x / s).max(MARGIN);
        let right = (print.max_x() / s).min(w - MARGIN);
        let width = (right - left).max(1.0);
        let dot = measure("·", &CAPTION_UPRIGHT);
        let line = |facts: &[String]| -> f64 {
            facts.iter().map(|f| measure(f, &CAPTION).width as f64).sum::<f64>() + facts.len().saturating_sub(1) as f64 * (2.0 * GAP + dot.width as f64)
        };
        let mut facts = words.facts.clone();
        // A line that can't fit loses its last facts.
        while facts.len() > 1 && line(&facts) > width {
            facts.pop();
        }
        let file = measure(&words.file, &CAPTION_UPRIGHT);
        let shows_file = !portrait && !words.file.is_empty() && line(&facts) + GAP + file.width as f64 <= width;
        let m = measure("Ag", &CAPTION);
        if m.height <= 0.0 {
            return;
        }
        // The line box's foot is 10 above the print.
        let baseline = print.y / s - 10.0 - (m.height - m.baseline) as f64;
        let mut x = if portrait { left + (width - line(&facts)) / 2.0 } else { left };
        for (i, fact) in facts.iter().enumerate() {
            if i > 0 {
                x += GAP;
                self.word(items, "·", CAPTION_UPRIGHT, ink.track, x, baseline);
                x += dot.width as f64 + GAP;
            }
            x += self.word(items, fact, CAPTION, ink.label, x, baseline).width as f64;
        }
        if shows_file {
            self.word(items, &words.file, CAPTION_UPRIGHT, ink.dim, right - file.width as f64, baseline);
        }
    }

    /// The label under `print` (pixels): "Film ⌄ printed on Paper ⌄", on one
    /// row when both fit under the print, else two, the paper's free to run
    /// past the print to the canvas's margin. Records where its names and
    /// the lock are, for clicks.
    pub(super) fn build_label(&mut self, items: &mut Vec<Draw>, print: Rect, ink: &SheetInk) {
        let s = self.scale();
        let words = self.scene.words.clone();
        let w = self.width as f64 / s;
        let left = (print.x / s).max(MARGIN);
        let right = (print.max_x() / s).min(w - MARGIN);
        let width = (right - left).max(1.0);
        let canvas_width = (w - MARGIN - left).max(width);
        let (scanning, original) = (words.scanning, words.original);
        let lockable = !scanning && !original;
        let kind = if scanning {
            "not printed — scanning the film"
        } else if lockable && words.keep_paper {
            "always printed on"
        } else {
            "printed on"
        };
        let opens = !original;
        let paper_opens = !original && !scanning;
        let film_m = measure(&words.film, &FILM);
        let paper_m = measure(&words.paper, &PAPER);
        if film_m.height <= 0.0 {
            return;
        }
        let caret = CARET.0 + 1.0;
        let film_width = film_m.width as f64 + INNER + caret;
        // Measured at its widest, so the lock never moves it between rows.
        let widest_kind = if lockable { "always printed on" } else { kind };
        let lock_width = if lockable { LOCK.0 + LOCK_SPACING } else { 0.0 };
        let paper_width = |kind: &str, name: f64| lock_width + measure(kind, &KIND).width as f64 + INNER + name + INNER + caret;
        let one_row = film_width + GAP + paper_width(widest_kind, paper_m.width as f64) <= width;
        let hot = self.hot;
        let top = print.max_y() / s;
        let (film_baseline, paper_baseline, paper_x) = if one_row {
            let baseline = top + 11.0 + film_m.baseline as f64;
            (baseline, baseline, left + film_width + GAP)
        } else {
            let film_baseline = top + 8.0 + film_m.baseline as f64;
            let row = top + 8.0 + film_m.height as f64 + 5.0;
            (film_baseline, row + paper_m.baseline as f64, left)
        };
        // The film.
        let film_color = if original { ink.dim } else { ink.paper };
        let film_room = if one_row { width } else { canvas_width } - INNER - caret;
        let film = fitted(&words.film, &FILM, film_room);
        let fm = self.word(items, &film, FILM, film_color, left, film_baseline);
        let film_rect = Rect::new(left * s, (film_baseline - fm.baseline as f64) * s, (fm.width as f64 + INNER + caret) * s, (fm.height as f64 + 5.0) * s);
        if opens {
            self.glyph(items, Glyph::Caret, ink.dim, 1.0, left + fm.width as f64 + INNER + 1.0, film_baseline - 6.0);
            self.targets.push((Target::Film, film_rect));
            if words.tray == Some(false) {
                self.open_mark(items, left, film_baseline - fm.baseline as f64 + fm.height as f64, fm.width as f64, ink);
            } else if hot == Some(Target::Film) {
                items.push(Draw::Fill(QuadInstance::fill(rect(left * s, (film_baseline - fm.baseline as f64 + fm.height as f64 + 4.0) * s, fm.width as f64 * s, 1.5 * s), hex(ink.track, 1.0))));
            }
        }
        if original {
            self.strike(items, left, film_baseline - fm.baseline as f64, fm.width as f64, fm.height as f64, 0.34, 1.0, ink);
        }
        // The lock and "printed on".
        let mut x = paper_x;
        if lockable {
            let lit = hot == Some(Target::Lock);
            let (lock_color, kind_color) = if words.keep_paper { (ink.accent, ink.label) } else if lit { (ink.label, ink.label) } else { (ink.track, ink.dim) };
            self.glyph(items, Glyph::Lock(words.keep_paper), lock_color, 1.0, x, paper_baseline - LOCK.1 + 1.0);
            let km = self.word(items, kind, KIND, kind_color, x + LOCK.0 + LOCK_SPACING, paper_baseline);
            let r = Rect::new(x * s, (paper_baseline - 14.0) * s, (LOCK.0 + LOCK_SPACING + km.width as f64) * s, 19.0 * s);
            self.targets.push((Target::Lock, r));
            x += LOCK.0 + LOCK_SPACING + km.width as f64 + INNER;
        } else {
            let km = self.word(items, kind, KIND, ink.dim, x, paper_baseline);
            x += km.width as f64 + INNER;
        }
        // The paper: it may run past the print, to the canvas's margin.
        let room = (w - MARGIN - x - INNER - caret - if original { 6.0 + measure("the original", &THE_ORIGINAL).width as f64 } else { 0.0 }).max(20.0);
        let paper = fitted(&words.paper, &PAPER, room);
        let paper_color = if original || scanning { ink.dim } else { ink.paper };
        let pm = self.word(items, &paper, PAPER, paper_color, x, paper_baseline);
        if paper_opens {
            self.glyph(items, Glyph::Caret, ink.dim, 1.0, x + pm.width as f64 + INNER + 1.0, paper_baseline - 6.0);
            let r = Rect::new(x * s, (paper_baseline - pm.baseline as f64) * s, (pm.width as f64 + INNER + caret) * s, (pm.height as f64 + 5.0) * s);
            self.targets.push((Target::Paper, r));
            if words.tray == Some(true) {
                self.open_mark(items, x, paper_baseline - pm.baseline as f64 + pm.height as f64, pm.width as f64, ink);
            } else if hot == Some(Target::Paper) {
                items.push(Draw::Fill(QuadInstance::fill(rect(x * s, (paper_baseline - pm.baseline as f64 + pm.height as f64 + 4.0) * s, pm.width as f64 * s, 1.5 * s), hex(ink.track, 1.0))));
            }
        }
        let paper_top = paper_baseline - pm.baseline as f64;
        if original {
            self.strike(items, x, paper_top, pm.width as f64, pm.height as f64, 0.34, 1.0, ink);
            self.word(items, "the original", THE_ORIGINAL, ink.paper, x + pm.width as f64 + 6.0, paper_baseline);
        } else if scanning {
            self.strike(items, x, paper_top, pm.width as f64, pm.height as f64, 0.54, 0.88, ink);
        }
    }

    /// The pencil under a name whose tray is open (`StockName`'s
    /// underline): 6 DIPs tall at the foot of the name's box and the 5 DIPs
    /// below it; Light table's highlighter, lifted behind the name's foot.
    fn open_mark(&mut self, items: &mut Vec<Draw>, x: f64, foot: f64, width: f64, ink: &SheetInk) {
        let w = (width * 10.0).round().max(10.0) as u32;
        if ink.highlighter {
            let reach = negs_marks::HIGHLIGHTER_WIDTH / 2.0;
            let ink_ = crate::ui::theme::HIGHLIGHTER_INK;
            let s = self.scale();
            items.push(Draw::Glyph(Glyph::Highlight(w + (20.0 * reach) as u32), hex(ink_, crate::ui::theme::HIGHLIGHTER_OPACITY), (((x - reach) * s).round(), ((foot - 5.0 - reach) * s).round())));
        } else {
            self.glyph(items, Glyph::Strike(w), ink.accent, 1.0, x, foot - 1.0);
        }
    }

    /// The pencil through a name: as CSS places it, `left at / width 6px`
    /// over the name and the 5 DIPs below it.
    #[allow(clippy::too_many_arguments)]
    fn strike(&mut self, items: &mut Vec<Draw>, x: f64, top: f64, width: f64, height: f64, at: f64, part: f64, ink: &SheetInk) {
        let w = (width * part * 10.0).round().max(10.0) as u32;
        let y = top + (height + 5.0 - 6.0) * at;
        self.glyph(items, Glyph::Strike(w), ink.accent, 1.0, x, y);
    }

    /// The trim marks beside `print` (pixels): `track`, `label` while the
    /// pointer is over the print, `paper` on the mark under it.
    pub(super) fn build_trim_marks(&mut self, items: &mut Vec<Draw>, print: Rect, ink: &SheetInk) {
        let s = self.scale();
        for (mark, handle) in trim_marks(print, s) {
            let hot = self.hot == Some(Target::Mark(handle));
            let color = if hot { ink.paper } else if self.over_print { ink.label } else { ink.track };
            items.push(Draw::Fill(QuadInstance::fill(rect(mark.x.round(), mark.y.round(), mark.width, mark.height), hex(color, 1.0))));
            self.targets.push((Target::Mark(handle), Rect::new(mark.x - 4.0 * s, mark.y - 9.0 * s, mark.width + 8.0 * s, mark.height + 18.0 * s)));
        }
    }

    /// The straightening dial (§4.10) under the photograph while cropping:
    /// the angle in Newsreader 15 `paper` over 61 ticks a degree apart,
    /// every fifth longer in `dim`, the others `track`, zero a 2 × 17 line
    /// in the pencil; a quarter-turn arrow at each end, `dim`, `paper`
    /// under the pointer.
    pub(super) fn build_straighten_dial(&mut self, items: &mut Vec<Draw>, frame: Rect, angle: f64, ink: &SheetInk) {
        let s = self.scale();
        let dial = dial_rect(frame, s);
        let decimal = crate::ui::inspector::decimal_separator();
        let label = degrees(angle, decimal);
        let m = measure(&label, &ANGLE);
        if m.height > 0.0 {
            let baseline = dial.y / s - 4.0 - (m.height - m.baseline) as f64;
            self.word(items, &label, ANGLE, ink.paper, dial.mid_x() / s - m.width as f64 / 2.0, baseline);
        }
        let mid = dial.x + dial.width / 2.0;
        let spacing = DIAL_SPACING * s;
        for degree in -75..=75 {
            let x = mid + (degree as f64 - angle) * spacing;
            if x < dial.x || x > dial.max_x() {
                continue;
            }
            let (zero, major) = (degree == 0, degree % 5 == 0);
            if zero {
                items.push(Draw::Fill(QuadInstance::rounded(rect(x - s, dial.y, 2.0 * s, 17.0 * s), hex(ink.accent, 1.0), s as f32, 0.0)));
            } else {
                let w = s.max(1.0).round();
                let h = if major { 11.0 } else { 6.0 } * s;
                items.push(Draw::Fill(QuadInstance::fill(rect((x - w / 2.0).round(), dial.y, w, h), hex(if major { ink.dim } else { ink.track }, 1.0))));
            }
        }
        for (r, clockwise) in turn_rects(dial, s) {
            let target = if clockwise { Target::TurnRight } else { Target::TurnLeft };
            let color = if self.hot == Some(target) { ink.paper } else { ink.dim };
            self.glyph(items, Glyph::Turn(clockwise), color, 1.0, r.x / s, r.y / s);
            self.targets.push((target, r));
        }
    }
}

impl View {
    /// What's under (x, y) pixels as last drawn, and whether that's over
    /// the print.
    pub(super) fn print_hit(&self, x: f64, y: f64) -> (Option<Target>, bool) {
        let p = Point { x, y };
        let target = self.targets.iter().find(|(_, r)| r.contains(p)).map(|(t, _)| *t);
        (target, self.print.is_some_and(|r| r.contains(p)))
    }

    /// Follows the pointer: the target it's over lights, the trim marks
    /// lift while it's over the print. True if anything changed.
    pub(super) fn hover_print(&mut self, at: Option<(f64, f64)>) -> bool {
        let (hot, over) = at.map_or((None, false), |(x, y)| self.print_hit(x, y));
        if (hot, over) == (self.hot, self.over_print) {
            return false;
        }
        self.hot = hot;
        self.over_print = over;
        invalidate(self.hwnd);
        if at.is_some() {
            let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: self.hwnd, dwHoverTime: 0 };
            unsafe {
                let _ = TrackMouseEvent(&mut track);
            }
        }
        true
    }

    /// Where a target was last drawn.
    pub(super) fn target_rect(&self, target: Target) -> Option<Rect> {
        self.targets.iter().find(|(t, _)| *t == target).map(|(_, r)| *r)
    }
}

/// A drag inward from a trim mark of more than this many DIPs crops, as a
/// click on it does.
pub(super) const MARK_DRAG: f64 = 4.0;

/// How far (x, y) has come from `start` toward the print's middle, for a
/// trim mark beside the `handle` corner.
pub(super) fn inward(handle: Handle, start: (f64, f64), x: f64, y: f64) -> f64 {
    let (dx, dy) = (x - start.0, y - start.1);
    (-(handle.x as f64) * dx).max(-(handle.y as f64) * dy)
}

/// A click on the print's words, a trim mark or a turn arrow.
pub(super) fn act(app: &mut App, hwnd: HWND, target: Target, at: Option<Rect>) {
    let model = &mut app.model;
    match target {
        Target::Film | Target::Paper => {
            let paper = target == Target::Paper;
            // Its tray, keeping the other's stock; a second click keeps
            // this one and closes it. Settings can turn the tiles off: then the menu.
            if model.can_open_tray(paper) {
                model.toggle_tray(paper);
            } else {
                choose_from_menu(app, hwnd, paper, at);
            }
        }
        Target::Lock => {
            let keep = !model.prefs.keep_paper;
            model.set_keep_paper(keep);
            model.dirty |= crate::model::dirty::CANVAS;
        }
        Target::Mark(_) => model.begin_crop(),
        Target::TurnLeft => model.turn(false),
        Target::TurnRight => model.turn(true),
    }
}

/// The film or paper as a menu under its name, with the tiles off.
fn choose_from_menu(app: &mut App, hwnd: HWND, paper: bool, at: Option<Rect>) {
    let entries = crate::ui::inspector::stock_entries(&app.model, paper);
    let current = if paper { app.model.state.paper.clone() } else { app.model.state.film.clone() };
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        for (i, (text, entry)) in entries.iter().enumerate() {
            let w = crate::platform::wide(&text.replace('&', "&&"));
            let flags = match entry {
                None => MF_STRING | MF_GRAYED,
                Some((id, favourite)) if *id == current && !favourite => MF_STRING | MF_CHECKED,
                Some(_) => MF_STRING,
            };
            let _ = AppendMenuW(menu, flags, 1 + i, crate::platform::pcwstr(&w));
        }
        let mut p = at.map_or(POINT::default(), |r| POINT { x: r.x as i32, y: r.max_y() as i32 });
        if at.is_none() {
            let _ = GetCursorPos(&mut p);
        } else {
            let _ = ClientToScreen(hwnd, &mut p);
        }
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN, p.x, p.y, 0, hwnd, None).0 as usize;
        let _ = DestroyMenu(menu);
        if let Some(Some((id, _))) = chosen.checked_sub(1).and_then(|i| entries.get(i)).map(|(_, e)| e.clone()) {
            if paper { app.model.choose_paper(&id) } else { app.model.choose_film(&id) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_marks_sit_beside_the_corners() {
        let marks = trim_marks(Rect::new(100.0, 50.0, 300.0, 200.0), 1.0);
        assert_eq!(marks[0].0, Rect::new(74.0, 50.0, 18.0, 1.5));
        assert_eq!(marks[1].0, Rect::new(408.0, 50.0, 18.0, 1.5));
        assert_eq!(marks[3].0, Rect::new(408.0, 248.5, 18.0, 1.5));
        assert_eq!(marks[2].1, Handle { x: -1, y: 1 });
    }

    #[test]
    fn crop_fits_the_print_place() {
        let (frame, scale) = crop_place(1000.0, 800.0, 1.0, Size::new(600.0, 400.0));
        assert!((scale - ((1000.0 - 96.0) / 600.0f64).min((800.0 - 122.0) / 400.0)).abs() < 1e-9);
        assert!(frame.y >= CAPTION_BAND && frame.max_y() <= 800.0 - LABEL_BAND + 0.5);
        assert!((frame.mid_x() - 500.0).abs() <= 0.5);
        let dial = dial_rect(frame, 1.0);
        assert_eq!(dial.width, DIAL_WIDTH);
        assert!(dial.y > frame.max_y());
    }

    #[test]
    fn degrees_use_the_readers_mark() {
        assert_eq!(degrees(0.0, ','), "0,0°");
        assert_eq!(degrees(-0.0, ','), "0,0°");
        assert_eq!(degrees(2.25, '.'), "2.2°");
    }
}
