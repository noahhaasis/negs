//! The print on the canvas (IMPLEMENTATION.md §4.1–§4.3, §4.10; ported from
//! `windows/src/ui/canvas/print.rs`): the caption over the photograph
//! (`Caption.swift`), the label under it (`PrintLabel.swift`) with the
//! paper lock, the trim marks beside its corners that start cropping
//! (`SmallStates.swift`), and while cropping the straightening dial in the
//! label's place (`Crop.swift`).
//!
//! Geometry is pure (DIPs, window coordinates) and unit-tested; painting
//! shapes words with GPUI's text system in the sheet's inks (`sheet.rs`) and
//! records where each name, the lock, the marks and the turn arrows are, for
//! the canvas's clicks.

use gpui::{App, Bounds, FontStyle, FontWeight, Hsla, PathBuilder, Pixels, SharedString, TextRun, Window, fill, font, point, px, size};
use negs_app::crop::{Handle, Point, Rect, Size};
use negs_marks::{Path, Pt, Seg};

use crate::marks_gpui::{paint_marks_path, paint_pencil};
use crate::sheet::SheetInk;
use crate::theme_gpui::{hex, hex_alpha};

/// Space round the print (§2.3): the side margins, the caption's band above
/// and the label's two rows below.
pub const MARGIN: f64 = 48.0;
pub const CAPTION_BAND: f64 = 54.0;
pub const LABEL_BAND: f64 = 68.0;

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
/// A drag inward from a trim mark of more than this many DIPs crops, as a
/// click on it does.
pub const MARK_DRAG: f64 = 4.0;

/// Newsreader's ascent and x-height, in ems, and the system font's x-height.
const NEWSREADER_ASCENT: f64 = 0.735;
const NEWSREADER_X_HEIGHT: f64 = 0.426;
const SYSTEM_X_HEIGHT: f64 = 0.5;

/// The straightening dial: 362 wide, 61 ticks a degree apart.
pub const DIAL_WIDTH: f64 = 362.0;
pub const DIAL_SPACING: f64 = (DIAL_WIDTH - 2.0) / 60.0;
const DIAL_HEIGHT: f64 = 18.0;

// MARK: Geometry

/// The band the photograph lies in, between the caption's band above and
/// the label's below; zoomed in, it scrolls under them.
pub fn band(canvas: Rect) -> Rect {
    Rect::new(canvas.x, canvas.y + CAPTION_BAND, canvas.width, (canvas.height - CAPTION_BAND - LABEL_BAND).max(1.0))
}

/// What a fitted print fills: the band less the side margins.
pub fn fit_room(canvas: Rect) -> (f64, f64) {
    let b = band(canvas);
    ((b.width - 2.0 * MARGIN).max(1.0), b.height)
}

/// A picture of shape `ratio` fitted as the canvas fits a print (never
/// enlarging past 100 % is ignored: thumbnails don't know the size).
pub fn fit_rect(canvas: Rect, ratio: Option<f64>) -> Option<Rect> {
    let ratio = ratio.filter(|r| *r > 0.0)?;
    if canvas.width <= 1.0 {
        return None;
    }
    let b = band(canvas);
    let (aw, ah) = fit_room(canvas);
    let (w, h) = if aw / ah > ratio { (ah * ratio, ah) } else { (aw, aw / ratio) };
    Some(Rect::new(b.x + (b.width - w) / 2.0, b.y + (b.height - h) / 2.0, w, h))
}

/// The trim marks beside `print`'s corners, level with its top and bottom
/// edges, with the corner each is beside.
pub fn trim_marks(print: Rect) -> [(Rect, Handle); 4] {
    let (top, bottom) = (print.y, print.max_y() - MARK_WEIGHT);
    let (left, right) = (print.x - MARK_REACH, print.max_x() + MARK_REACH - MARK_LENGTH);
    [
        (Rect::new(left, top, MARK_LENGTH, MARK_WEIGHT), Handle { x: -1, y: -1 }),
        (Rect::new(right, top, MARK_LENGTH, MARK_WEIGHT), Handle { x: 1, y: -1 }),
        (Rect::new(left, bottom, MARK_LENGTH, MARK_WEIGHT), Handle { x: -1, y: 1 }),
        (Rect::new(right, bottom, MARK_LENGTH, MARK_WEIGHT), Handle { x: 1, y: 1 }),
    ]
}

/// Where a trim mark takes clicks.
pub fn mark_target(mark: Rect) -> Rect {
    Rect::new(mark.x - 4.0, mark.y - 9.0, mark.width + 8.0, mark.height + 18.0)
}

/// The photograph's place while cropping, as the print's: fitted between
/// the caption's and the label's bands and the side margins
/// (`CropEditor.place`). Returns the frame and DIPs per photo pixel.
pub fn crop_place(canvas: Rect, photo: Size) -> (Rect, f64) {
    let room = Size::new((canvas.width - 2.0 * MARGIN).max(1.0), (canvas.height - CAPTION_BAND - LABEL_BAND).max(1.0));
    let scale = if photo.width > 0.0 && photo.height > 0.0 { (room.width / photo.width).min(room.height / photo.height) } else { 1.0 };
    let fitted = Size::new(photo.width * scale, photo.height * scale);
    let frame = Rect::new(
        canvas.x + ((canvas.width - fitted.width) / 2.0).round(),
        canvas.y + (CAPTION_BAND + (room.height - fitted.height) / 2.0).round(),
        fitted.width,
        fitted.height,
    );
    (frame, scale)
}

/// The dial's tick row under `frame`, 10 DIPs below it, with its angle over it.
pub fn dial_rect(frame: Rect) -> Rect {
    let angle_height = 20.0;
    Rect::new((frame.mid_x() - DIAL_WIDTH / 2.0).round(), (frame.max_y() + 10.0 + angle_height + 4.0).round(), DIAL_WIDTH, DIAL_HEIGHT)
}

/// The quarter-turn arrows either side of the dial: 24 DIPs square, 34
/// outside its ends, 4 above its ticks. `true` turns clockwise.
pub fn turn_rects(dial: Rect) -> [(Rect, bool); 2] {
    let s = 24.0;
    [(Rect::new(dial.x - 34.0, dial.y - 4.0, s, s), false), (Rect::new(dial.max_x() + 34.0 - s, dial.y - 4.0, s, s), true)]
}

/// "0.0°", in the reader's decimal mark.
pub fn degrees(angle: f64, decimal: char) -> String {
    let angle = if angle == 0.0 { 0.0 } else { angle };
    format!("{angle:.1}°").replace('.', &decimal.to_string())
}

/// How far (x, y) has come from `start` toward the print's middle, for a
/// trim mark beside the `handle` corner.
pub fn inward(handle: Handle, start: (f64, f64), x: f64, y: f64) -> f64 {
    let (dx, dy) = (x - start.0, y - start.1);
    (-(handle.x as f64) * dx).max(-(handle.y as f64) * dy)
}

/// What a click on the canvas can reach, outside cropping's frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    Film,
    Paper,
    Lock,
    /// The film a negative is read as.
    ReadAs,
    /// A trim mark, by the corner it's beside.
    Mark(Handle),
    TurnLeft,
    TurnRight,
}

/// The first target containing `p`.
pub fn hit(targets: &[(Target, Rect)], p: Point) -> Option<Target> {
    targets.iter().find(|(_, r)| r.contains(p)).map(|(t, _)| *t)
}

/// What the caption and label say, from the model.
#[derive(Clone, Default, PartialEq, Debug)]
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
    /// Read as a negative: "Your negative read as <film>" on a row of its own.
    pub negative: bool,
    /// The tray of films the negative is read as is open.
    pub read_as_open: bool,
    /// Prep's picker waits for a click: its prompt takes the paper's row.
    pub prompt: Option<&'static str>,
}

/// The label's words in order (`kind` is "printed on", "always printed
/// on" or the scanning note).
pub fn label_kind(words: &PrintWords) -> &'static str {
    if words.scanning {
        "not printed — scanning the film"
    } else if !words.original && words.keep_paper {
        "always printed on"
    } else {
        "printed on"
    }
}

// MARK: Painting

/// A text style: size, weight, italic, in the serif (Newsreader, or the
/// system font in Plain).
#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub size: f32,
    pub weight: f32,
    pub italic: bool,
}

impl TextStyle {
    pub const fn serif(size: f32, weight: f32) -> Self {
        Self { size, weight, italic: false }
    }
    pub const fn italic(self) -> Self {
        Self { italic: true, ..self }
    }
}

pub const CAPTION: TextStyle = TextStyle::serif(15.0, 400.0).italic();
pub const CAPTION_UPRIGHT: TextStyle = TextStyle::serif(15.0, 400.0);
pub const FILM: TextStyle = TextStyle::serif(24.0, 600.0);
pub const KIND: TextStyle = TextStyle::serif(14.0, 400.0).italic();
pub const PAPER: TextStyle = TextStyle::serif(17.5, 400.0);
const THE_ORIGINAL: TextStyle = TextStyle::serif(16.0, 400.0).italic();
const ANGLE: TextStyle = TextStyle::serif(15.0, 400.0);

/// Measured words: width, line height and the baseline below the line's top.
#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics {
    pub width: f64,
    pub height: f64,
    pub baseline: f64,
}

/// Words and marks on the canvas, in the theme's serif.
pub struct Painter<'a> {
    pub window: &'a mut Window,
    pub cx: &'a mut App,
    pub serif: SharedString,
    pub plain: bool,
}

impl Painter<'_> {
    fn shape(&mut self, text: &str, style: &TextStyle, color: Hsla) -> gpui::ShapedLine {
        let f = gpui::Font {
            style: if style.italic { FontStyle::Italic } else { FontStyle::Normal },
            weight: FontWeight(style.weight),
            ..font(self.serif.clone())
        };
        let run = TextRun { len: text.len(), font: f, color, background_color: None, underline: None, strikethrough: None };
        self.window.text_system().shape_line(SharedString::from(text.to_string()), px(style.size), &[run], None)
    }

    pub fn measure(&mut self, text: &str, style: &TextStyle) -> Metrics {
        let line = self.shape(text, style, gpui::black());
        let (ascent, descent) = (f32::from(line.ascent) as f64, f32::from(line.descent) as f64);
        Metrics { width: f32::from(line.width) as f64, height: ascent + descent, baseline: ascent }
    }

    /// Words with their baseline at `baseline` and their left at `x`.
    pub fn word(&mut self, text: &str, style: TextStyle, color: Hsla, x: f64, baseline: f64) -> Metrics {
        let line = self.shape(text, &style, color);
        let (ascent, descent) = (f32::from(line.ascent), f32::from(line.descent));
        let m = Metrics { width: f32::from(line.width) as f64, height: (ascent + descent) as f64, baseline: ascent as f64 };
        if m.width > 0.0 {
            let _ = line.paint(point(px(x as f32), px(baseline as f32 - ascent)), px(ascent + descent), self.window, self.cx);
        }
        m
    }

    /// `text` cut with an ellipsis to fit `room` DIPs.
    pub fn fitted(&mut self, text: &str, style: &TextStyle, room: f64) -> String {
        if self.measure(text, style).width <= room {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        let (mut low, mut high) = (0, chars.len());
        while low < high {
            let mid = (low + high).div_ceil(2);
            let candidate: String = chars[..mid].iter().collect::<String>().trim_end().to_string() + "…";
            if self.measure(&candidate, style).width <= room {
                low = mid
            } else {
                high = mid - 1
            }
        }
        chars[..low].iter().collect::<String>().trim_end().to_string() + "…"
    }

    pub fn fill(&mut self, r: Rect, color: Hsla) {
        self.window.paint_quad(fill(bounds(r), color));
    }

    /// The pencil through or under a name, `w` DIPs long, 6 tall.
    fn strike_mark(&mut self, x: f64, y: f64, w: f64, color: Hsla) {
        paint_pencil(self.window, bounds(Rect::new(x, y, w.max(1.0), 6.0)), self.plain, negs_marks::PENCIL_WIDTH as f32, color);
    }
}

pub fn bounds(r: Rect) -> Bounds<Pixels> {
    Bounds { origin: point(px(r.x as f32), px(r.y as f32)), size: size(px(r.width as f32), px(r.height as f32)) }
}

/// The `⌄` after a stock's name, its box's top left at (x, y).
pub fn paint_caret(window: &mut Window, x: f64, y: f64, color: Hsla) {
    let mut b = PathBuilder::stroke(px(1.15));
    let at = |dx: f64, dy: f64| point(px((x + dx) as f32), px((y + dy) as f32));
    b.move_to(at(1.5, 2.5));
    b.line_to(at(4.5, 5.5));
    b.line_to(at(7.5, 2.5));
    if let Ok(path) = b.build() {
        window.paint_path(path, color);
    }
}

/// The paper lock, `LOCK` DIPs, its top left at (x, y): a body with a
/// shackle over it, swung up and right when open.
pub fn paint_lock(window: &mut Window, closed: bool, x: f64, y: f64, color: Hsla) {
    window.paint_quad(fill(bounds(Rect::new(x + 1.0, y + 5.5, 8.0, 6.0)), color).corner_radii(px(1.4)));
    let (left, right, top, foot) = if closed { (2.9, 7.1, 1.2, 5.8) } else { (5.4, 9.6, 0.2, 5.8) };
    let r = (right - left) / 2.0;
    let k = 0.5523 * r;
    let left_foot = if closed { foot } else { 3.4 };
    let shackle = Path(vec![
        Seg::Move(Pt { x: left, y: left_foot }),
        Seg::Line(Pt { x: left, y: top + r }),
        Seg::Cubic(Pt { x: left, y: top + r - k }, Pt { x: left + r - k, y: top }, Pt { x: left + r, y: top }),
        Seg::Cubic(Pt { x: left + r + k, y: top }, Pt { x: right, y: top + r - k }, Pt { x: right, y: top + r }),
        Seg::Line(Pt { x: right, y: foot }),
    ]);
    paint_marks_path(window, &shackle, point(px(x as f32), px(y as f32)), Some(1.3), color);
}

/// A quarter-turn arrow (`arrow.clockwise` at 14 pt) in a 24-DIP box.
fn paint_turn(window: &mut Window, clockwise: bool, x: f64, y: f64, color: Hsla) {
    let (cx, cy, r) = (12.0, 12.5, 5.6);
    let dir = if clockwise { 1.0 } else { -1.0 };
    let (start, sweep, steps) = (-90.0f64, 300.0f64, 5);
    let at = |deg: f64| Pt { x: cx + r * deg.to_radians().cos(), y: cy + r * deg.to_radians().sin() };
    let mut segs = vec![Seg::Move(at(start + dir * 20.0))];
    let step = sweep / steps as f64;
    for i in 0..steps {
        let a0 = (start + dir * (20.0 + step * i as f64)).to_radians();
        let a1 = (start + dir * (20.0 + step * (i + 1) as f64)).to_radians();
        let k = 4.0 / 3.0 * ((a1 - a0) / 4.0).tan() * r;
        let p0 = (cx + r * a0.cos(), cy + r * a0.sin());
        let p1 = (cx + r * a1.cos(), cy + r * a1.sin());
        segs.push(Seg::Cubic(Pt { x: p0.0 - k * a0.sin(), y: p0.1 + k * a0.cos() }, Pt { x: p1.0 + k * a1.sin(), y: p1.1 - k * a1.cos() }, Pt { x: p1.0, y: p1.1 }));
    }
    let origin = point(px(x as f32), px(y as f32));
    paint_marks_path(window, &Path(segs), origin, Some(1.3), color);
    let (hx, hy) = (12.0 + 5.6 * (-70.0f64).to_radians().cos() * dir, 12.5 + 5.6 * (-70.0f64).to_radians().sin());
    let head = Path::polygon(&[Pt { x: hx - dir * 3.6, y: hy - 2.9 }, Pt { x: hx + dir * 1.4, y: hy - 0.2 }, Pt { x: hx - dir * 2.6, y: hy + 3.0 }]);
    paint_marks_path(window, &head, origin, None, color);
}

/// The caption over `print`: the facts in italic `label`, `·` between them
/// in `track`, the file name at the right in `dim`, its line's foot 10 DIPs
/// above the print. The file name goes first when they don't fit; a
/// portrait's caption is centred and has none.
pub fn paint_caption(p: &mut Painter, canvas: Rect, print: Rect, portrait: bool, words: &PrintWords, ink: &SheetInk) {
    let left = print.x.max(canvas.x + MARGIN);
    let right = print.max_x().min(canvas.max_x() - MARGIN);
    let width = (right - left).max(1.0);
    let dot = p.measure("·", &CAPTION_UPRIGHT).width;
    let widths: Vec<f64> = words.facts.iter().map(|f| p.measure(f, &CAPTION).width).collect();
    let line = |n: usize| widths[..n].iter().sum::<f64>() + n.saturating_sub(1) as f64 * (2.0 * GAP + dot);
    let mut n = words.facts.len();
    while n > 1 && line(n) > width {
        n -= 1;
    }
    let file = p.measure(&words.file, &CAPTION_UPRIGHT);
    let shows_file = !portrait && !words.file.is_empty() && line(n) + GAP + file.width <= width;
    let m = p.measure("Ag", &CAPTION);
    if m.height <= 0.0 {
        return;
    }
    let baseline = print.y - 10.0 - (m.height - m.baseline);
    let mut x = if portrait { left + (width - line(n)) / 2.0 } else { left };
    for (i, fact) in words.facts[..n].iter().enumerate() {
        if i > 0 {
            x += GAP;
            p.word("·", CAPTION_UPRIGHT, hex(ink.track), x, baseline);
            x += dot + GAP;
        }
        x += p.word(fact, CAPTION, hex(ink.label), x, baseline).width;
    }
    if shows_file {
        p.word(&words.file, CAPTION_UPRIGHT, hex(ink.dim), right - file.width, baseline);
    }
}

/// The pencil under a name whose tray is open (`StockName`'s underline);
/// Light table's highlighter, lifted behind the name's foot.
fn open_mark(p: &mut Painter, x: f64, foot: f64, width: f64, ink: &SheetInk) {
    if ink.highlighter {
        let reach = negs_marks::HIGHLIGHTER_WIDTH / 2.0;
        let color = hex_alpha(negs_app::theme::HIGHLIGHTER_INK, negs_app::theme::HIGHLIGHTER_OPACITY);
        let r = Rect::new(x, foot - 5.0, width.max(1.0), 6.0);
        paint_pencil(p.window, bounds(r), p.plain, negs_marks::HIGHLIGHTER_WIDTH as f32, color);
        let _ = reach;
    } else {
        p.strike_mark(x, foot - 1.0, width, hex(ink.accent));
    }
}

/// The pencil through a name (`m`, `size` DIPs, its baseline at `baseline`):
/// Original's strike at 0.34, scanning's lower strike at 0.54 of the line.
#[allow(clippy::too_many_arguments)]
fn strike(p: &mut Painter, x: f64, baseline: f64, m: &Metrics, size: f32, at: f64, part: f64, ink: &SheetInk) {
    let w = m.width * part;
    let y = if p.plain && at < 0.5 {
        let size = size as f64;
        let over = NEWSREADER_ASCENT * size - ((size + 5.0 - 6.0) * at + 3.0);
        baseline - over * SYSTEM_X_HEIGHT / NEWSREADER_X_HEIGHT - 3.0
    } else {
        baseline - m.baseline + (m.height + 5.0 - 6.0) * at
    };
    p.strike_mark(x, y, w, hex(ink.accent));
}

/// The label under `print`: "Film ⌄ printed on Paper ⌄", on one row when
/// both fit under the print, else two, the paper's free to run past the
/// print to the canvas's margin. Returns where its names and the lock are.
pub fn paint_label(p: &mut Painter, canvas: Rect, print: Rect, words: &PrintWords, ink: &SheetInk, hot: Option<Target>) -> Vec<(Target, Rect)> {
    let mut targets = vec![];
    let left = print.x.max(canvas.x + MARGIN);
    let right = print.max_x().min(canvas.max_x() - MARGIN);
    let width = (right - left).max(1.0);
    let canvas_width = (canvas.max_x() - MARGIN - left).max(width);
    let (scanning, original) = (words.scanning, words.original);
    let lockable = !scanning && !original;
    let kind = label_kind(words);
    let opens = !original;
    let paper_opens = !original && !scanning;
    let film_m = p.measure(&words.film, &FILM);
    let paper_m = p.measure(&words.paper, &PAPER);
    if film_m.height <= 0.0 {
        return targets;
    }
    let caret = CARET.0 + 1.0;
    let film_width = film_m.width + INNER + caret;
    // Measured at its widest, so the lock never moves it between rows.
    let widest_kind = if lockable { "always printed on" } else { kind };
    let lock_width = if lockable { LOCK.0 + LOCK_SPACING } else { 0.0 };
    let kind_width = p.measure(widest_kind, &KIND).width;
    let paper_width = lock_width + kind_width + INNER + paper_m.width + INNER + caret;
    // A negative always takes two rows: "Your negative, read as <film>", then the paper.
    let one_row = !words.negative && film_width + GAP + paper_width <= width;
    let top = print.max_y();
    let (film_baseline, paper_baseline, paper_x) = if one_row {
        let baseline = top + 11.0 + film_m.baseline;
        (baseline, baseline, left + film_width + GAP)
    } else {
        let film_baseline = top + 8.0 + film_m.baseline;
        let row = top + 8.0 + film_m.height + 5.0;
        (film_baseline, row + paper_m.baseline, left)
    };
    let film_color = if original { ink.dim } else { ink.paper };
    let film_room = if one_row { width } else { canvas_width } - INNER - caret;
    let film = if words.negative { negs_app::negative::NAME.to_string() } else { p.fitted(&words.film, &FILM, film_room) };
    let fm = p.word(&film, FILM, hex(film_color), left, film_baseline);
    let film_rect = Rect::new(left, film_baseline - fm.baseline, fm.width + INNER + caret, fm.height + 5.0);
    if opens {
        paint_caret(p.window, left + fm.width + INNER + 1.0, film_baseline - 6.0, hex(ink.dim));
        targets.push((Target::Film, film_rect));
        if words.tray == Some(false) {
            open_mark(p, left, film_baseline - fm.baseline + fm.height, fm.width, ink);
        } else if hot == Some(Target::Film) {
            p.fill(Rect::new(left, film_baseline - fm.baseline + fm.height + 4.0, fm.width, 1.5), hex(ink.track));
        }
    }
    if original {
        strike(p, left, film_baseline, &fm, FILM.size, 0.34, 1.0, ink);
    }
    if words.negative {
        // "read as" and the film, which opens the colour negatives.
        let x = left + fm.width + INNER + caret + INNER;
        let km = p.word("read as", KIND, hex(ink.dim), x, film_baseline);
        let x = x + km.width + INNER;
        let room = (canvas.max_x() - MARGIN - x - INNER - caret).max(20.0);
        let name = p.fitted(&words.film, &PAPER, room);
        let nm = p.word(&name, PAPER, hex(film_color), x, film_baseline);
        let name_top = film_baseline - nm.baseline;
        if opens {
            paint_caret(p.window, x + nm.width + INNER + 1.0, film_baseline - 6.0, hex(ink.dim));
            targets.push((Target::ReadAs, Rect::new(x, name_top, nm.width + INNER + caret, nm.height + 5.0)));
            if words.read_as_open {
                open_mark(p, x, name_top + nm.height, nm.width, ink);
            } else if hot == Some(Target::ReadAs) {
                p.fill(Rect::new(x, name_top + nm.height + 4.0, nm.width, 1.5), hex(ink.track));
            }
        }
        if original {
            strike(p, x, film_baseline, &nm, PAPER.size, 0.34, 1.0, ink);
        }
    }
    // The canvas waits for a click (Prep's pickers): the prompt takes the paper's row.
    if let Some(prompt) = words.prompt.filter(|_| !one_row) {
        p.word(prompt, KIND, hex(ink.accent), left, paper_baseline);
        return targets;
    }
    let mut x = paper_x;
    if lockable {
        let lit = hot == Some(Target::Lock);
        let (lock_color, kind_color) = if words.keep_paper {
            (ink.accent, ink.label)
        } else if lit {
            (ink.label, ink.label)
        } else {
            (ink.track, ink.dim)
        };
        paint_lock(p.window, words.keep_paper, x, paper_baseline - LOCK.1 + 1.0, hex(lock_color));
        let km = p.word(kind, KIND, hex(kind_color), x + LOCK.0 + LOCK_SPACING, paper_baseline);
        targets.push((Target::Lock, Rect::new(x, paper_baseline - 14.0, LOCK.0 + LOCK_SPACING + km.width, 19.0)));
        x += LOCK.0 + LOCK_SPACING + km.width + INNER;
    } else {
        let km = p.word(kind, KIND, hex(ink.dim), x, paper_baseline);
        x += km.width + INNER;
    }
    let original_width = if original { 6.0 + p.measure("the original", &THE_ORIGINAL).width } else { 0.0 };
    let room = (canvas.max_x() - MARGIN - x - INNER - caret - original_width).max(20.0);
    let paper = p.fitted(&words.paper, &PAPER, room);
    let paper_color = if original || scanning { ink.dim } else { ink.paper };
    let pm = p.word(&paper, PAPER, hex(paper_color), x, paper_baseline);
    if paper_opens {
        paint_caret(p.window, x + pm.width + INNER + 1.0, paper_baseline - 6.0, hex(ink.dim));
        targets.push((Target::Paper, Rect::new(x, paper_baseline - pm.baseline, pm.width + INNER + caret, pm.height + 5.0)));
        if words.tray == Some(true) {
            open_mark(p, x, paper_baseline - pm.baseline + pm.height, pm.width, ink);
        } else if hot == Some(Target::Paper) {
            p.fill(Rect::new(x, paper_baseline - pm.baseline + pm.height + 4.0, pm.width, 1.5), hex(ink.track));
        }
    }
    if original {
        strike(p, x, paper_baseline, &pm, PAPER.size, 0.34, 1.0, ink);
        p.word("the original", THE_ORIGINAL, hex(ink.paper), x + pm.width + 6.0, paper_baseline);
    } else if scanning {
        strike(p, x, paper_baseline, &pm, PAPER.size, 0.54, 0.88, ink);
    }
    targets
}

/// The trim marks beside `print`: `track`, `label` while the pointer is
/// over the print, `paper` on the mark under it.
pub fn paint_trim_marks(window: &mut Window, print: Rect, ink: &SheetInk, hot: Option<Target>, over_print: bool) -> Vec<(Target, Rect)> {
    let mut targets = vec![];
    for (mark, handle) in trim_marks(print) {
        let lit = hot == Some(Target::Mark(handle));
        let color = if lit { ink.paper } else if over_print { ink.label } else { ink.track };
        window.paint_quad(fill(bounds(mark), hex(color)));
        targets.push((Target::Mark(handle), mark_target(mark)));
    }
    targets
}

/// The straightening dial under the photograph while cropping: the angle
/// over 61 ticks a degree apart, every fifth longer in `dim`, the others
/// `track`, zero a 2 × 17 line in the pencil; a quarter-turn arrow at each
/// end, `dim`, `paper` under the pointer.
pub fn paint_dial(p: &mut Painter, frame: Rect, angle: f64, ink: &SheetInk, hot: Option<Target>, decimal: char) -> Vec<(Target, Rect)> {
    let dial = dial_rect(frame);
    let label = degrees(angle, decimal);
    let m = p.measure(&label, &ANGLE);
    if m.height > 0.0 {
        let baseline = dial.y - 4.0 - (m.height - m.baseline);
        p.word(&label, ANGLE, hex(ink.paper), dial.mid_x() - m.width / 2.0, baseline);
    }
    let mid = dial.x + dial.width / 2.0;
    for degree in -75..=75 {
        let x = mid + (degree as f64 - angle) * DIAL_SPACING;
        if x < dial.x || x > dial.max_x() {
            continue;
        }
        let (zero, major) = (degree == 0, degree % 5 == 0);
        if zero {
            p.window.paint_quad(fill(bounds(Rect::new(x - 1.0, dial.y, 2.0, 17.0)), hex(ink.accent)).corner_radii(px(1.0)));
        } else {
            let h = if major { 11.0 } else { 6.0 };
            p.fill(Rect::new(x - 0.5, dial.y, 1.0, h), hex(if major { ink.dim } else { ink.track }));
        }
    }
    let mut targets = vec![];
    for (r, clockwise) in turn_rects(dial) {
        let target = if clockwise { Target::TurnRight } else { Target::TurnLeft };
        let color = if hot == Some(target) { ink.paper } else { ink.dim };
        paint_turn(p.window, clockwise, r.x, r.y, hex(color));
        targets.push((target, r));
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_marks_sit_beside_the_corners() {
        let print = Rect::new(100.0, 50.0, 300.0, 200.0);
        let marks = trim_marks(print);
        assert_eq!(marks[0].0, Rect::new(74.0, 50.0, 18.0, 1.5));
        assert_eq!(marks[3].0, Rect::new(408.0, 248.5, 18.0, 1.5));
        assert_eq!(marks[1].1, Handle { x: 1, y: -1 });
        // Clicks reach a little round each.
        assert!(mark_target(marks[0].0).contains(Point { x: 80.0, y: 58.0 }));
    }

    #[test]
    fn crop_fits_the_print_place_and_the_dial_sits_under_it() {
        let canvas = Rect::new(0.0, 38.0, 1000.0, 700.0);
        let (frame, scale) = crop_place(canvas, Size::new(6000.0, 4000.0));
        // 904 × 578 of room: the height binds.
        assert!((scale - 578.0 / 4000.0).abs() < 1e-9);
        assert!((frame.height - 578.0).abs() < 1e-9);
        assert_eq!(frame.y, 38.0 + 54.0);
        assert!((frame.mid_x() - 500.0).abs() < 1.0);
        let dial = dial_rect(frame);
        assert_eq!(dial.width, DIAL_WIDTH);
        assert!(dial.y > frame.max_y() + 30.0);
        let [(left, false), (right, true)] = turn_rects(dial) else { panic!() };
        assert!(left.max_x() < dial.x && right.x > dial.max_x());
    }

    #[test]
    fn the_band_and_fit_leave_room_for_caption_and_label() {
        let canvas = Rect::new(10.0, 38.0, 800.0, 600.0);
        let b = band(canvas);
        assert_eq!((b.y, b.height), (38.0 + 54.0, 600.0 - 54.0 - 68.0));
        assert_eq!(fit_room(canvas), (800.0 - 96.0, 478.0));
        let r = fit_rect(canvas, Some(1.5)).unwrap();
        assert!((r.width / r.height - 1.5).abs() < 1e-9);
        assert!(r.width <= 704.0 + 1e-9 && r.height <= 478.0 + 1e-9);
        assert!((r.mid_x() - b.mid_x()).abs() < 1e-9 && (r.mid_y() - b.mid_y()).abs() < 1e-9);
    }

    #[test]
    fn degrees_use_the_readers_mark_and_drag_inward_counts() {
        assert_eq!(degrees(-0.0, '.'), "0.0°");
        assert_eq!(degrees(1.5, ','), "1,5°");
        // From the top-left mark, right or down is inward.
        assert_eq!(inward(Handle { x: -1, y: -1 }, (0.0, 0.0), 5.0, 2.0), 5.0);
        assert_eq!(inward(Handle { x: 1, y: 1 }, (0.0, 0.0), 5.0, 2.0), -2.0);
    }

    #[test]
    fn the_label_kind_follows_lock_scan_and_original() {
        let mut w = PrintWords { keep_paper: true, ..Default::default() };
        assert_eq!(label_kind(&w), "always printed on");
        w.original = true;
        assert_eq!(label_kind(&w), "printed on");
        w.scanning = true;
        assert_eq!(label_kind(&w), "not printed — scanning the film");
    }
}
