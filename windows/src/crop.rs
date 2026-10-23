//! A presentation crop, like the border: changing one never re-renders. The
//! rectangle is a fraction of the unrotated photograph, origin top left, so
//! it turns with the photograph. Straightening turns the photograph by
//! `angle` degrees (clockwise as shown) about its centre, and the rectangle
//! is then in that turned frame. Export and Save Preview crop with
//! `spektrafilm-io::crop`, which this module calls for the pixel rounding.
//! Mirrors `Crop.swift` in the macOS app.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const MAX_ANGLE: f64 = spektrafilm_io::crop::MAX_ANGLE;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub const ZERO: Size = Size { width: 0.0, height: 0.0 };
    pub fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
    pub fn swapped(self) -> Self {
        Self { width: self.height, height: self.width }
    }
    pub fn is_zero(self) -> bool {
        self.width == 0.0 && self.height == 0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const UNIT: Rect = Rect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 };
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }
    pub fn of_size(size: Size) -> Self {
        Self::new(0.0, 0.0, size.width, size.height)
    }
    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }
    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }
    pub fn mid_x(&self) -> f64 {
        self.x + self.width / 2.0
    }
    pub fn mid_y(&self) -> f64 {
        self.y + self.height / 2.0
    }
    pub fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
    pub fn inset(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width - 2.0 * dx, self.height - 2.0 * dy)
    }
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.max_x() && p.y >= self.y && p.y < self.max_y()
    }
    pub fn intersection(&self, other: &Rect) -> Rect {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.max_x().min(other.max_x());
        let y1 = self.max_y().min(other.max_y());
        Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
    }
    pub fn offset(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }
}

/// `[[x, y], [width, height]]`, as the macOS app's session file writes a CGRect.
impl Serialize for Rect {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        [[self.x, self.y], [self.width, self.height]].serialize(s)
    }
}
impl<'de> Deserialize<'de> for Rect {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let [[x, y], [width, height]] = <[[f64; 2]; 2]>::deserialize(d)?;
        Ok(Rect { x, y, width, height })
    }
}
impl Serialize for Size {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        [self.width, self.height].serialize(s)
    }
}
impl<'de> Deserialize<'de> for Size {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let [width, height] = <[f64; 2]>::deserialize(d)?;
        Ok(Size { width, height })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CropAspect {
    #[default]
    #[serde(rename = "original")]
    Original,
    #[serde(rename = "freeform")]
    Freeform,
    #[serde(rename = "square")]
    Square,
    #[serde(rename = "16:9")]
    SixteenNine,
    #[serde(rename = "3:2")]
    ThreeTwo,
    #[serde(rename = "4:3")]
    FourThree,
    #[serde(rename = "5:4")]
    FiveFour,
    #[serde(rename = "7:5")]
    SevenFive,
    #[serde(rename = "custom")]
    Custom,
}

impl CropAspect {
    pub const ALL: [CropAspect; 9] = [
        Self::Original,
        Self::Freeform,
        Self::Square,
        Self::SixteenNine,
        Self::ThreeTwo,
        Self::FourThree,
        Self::FiveFour,
        Self::SevenFive,
        Self::Custom,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Freeform => "Freeform",
            Self::Square => "Square",
            Self::SixteenNine => "16:9",
            Self::ThreeTwo => "3:2",
            Self::FourThree => "4:3",
            Self::FiveFour => "5:4",
            Self::SevenFive => "7:5",
            Self::Custom => "Custom",
        }
    }

    /// The film or print format with that shape.
    pub fn format(self) -> Option<&'static str> {
        match self {
            Self::Square => Some("6×6"),
            Self::ThreeTwo => Some("35mm"),
            Self::FourThree => Some("645"),
            Self::FiveFour => Some("4×5"),
            Self::SevenFive => Some("5×7"),
            _ => None,
        }
    }

    /// The long side over the short side, for a photograph shown at `photo`.
    pub fn ratio(self, photo: Size) -> Option<f64> {
        match self {
            Self::Freeform | Self::Custom => None,
            Self::Original => (photo.width > 0.0 && photo.height > 0.0)
                .then(|| photo.width.max(photo.height) / photo.width.min(photo.height)),
            Self::Square => Some(1.0),
            Self::SixteenNine => Some(16.0 / 9.0),
            Self::ThreeTwo => Some(1.5),
            Self::FourThree => Some(4.0 / 3.0),
            Self::FiveFour => Some(1.25),
            Self::SevenFive => Some(1.4),
        }
    }

    /// Whether the ratio has a landscape and a portrait orientation.
    pub fn turns(self) -> bool {
        self != Self::Freeform && self != Self::Square
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CropSettings {
    pub rect: Rect,
    /// The aspect ratio the rectangle was drawn with, offered again next time.
    pub aspect: CropAspect,
    /// The custom ratio, width by height as shown.
    pub custom: Size,
    pub angle: f64,
}

impl Default for CropSettings {
    fn default() -> Self {
        Self { rect: Rect::UNIT, aspect: CropAspect::Original, custom: Size::new(5.0, 4.0), angle: 0.0 }
    }
}

impl CropSettings {
    pub fn cropped(&self) -> bool {
        self.rect != Rect::UNIT || self.angle != 0.0
    }

    pub fn engine(&self) -> spektrafilm_io::crop::Crop {
        spektrafilm_io::crop::Crop { x: self.rect.x, y: self.rect.y, width: self.rect.width, height: self.rect.height, angle: self.angle }
    }

    /// The pixel rectangle in a `size` photograph, rounded like the
    /// exporter: edges round to the nearest pixel, and at least one pixel
    /// remains. A straightened crop keeps its origin and rounds its sides.
    pub fn pixels(&self, size: Size) -> Rect {
        if !self.cropped() {
            return Rect::of_size(size);
        }
        let (w, h) = (size.width.round() as u32, size.height.round() as u32);
        if w == 0 || h == 0 {
            return Rect::of_size(size);
        }
        let crop = self.engine();
        if self.angle != 0.0 {
            let (sw, sh) = crop.straightened_size(w, h);
            return Rect::new(self.rect.x * w as f64, self.rect.y * h as f64, sw as f64, sh as f64);
        }
        let (x, y, pw, ph) = crop.pixels(w, h);
        Rect::new(x as f64, y as f64, pw as f64, ph as f64)
    }

    /// The cropped photograph's size, before rotation.
    pub fn size_of(&self, size: Size) -> Size {
        if size.is_zero() { Size::ZERO } else { self.pixels(size).size() }
    }

    /// The rectangle as the photograph is shown, after `rotation` clockwise
    /// quarter turns, like `ImageBuf::rotated_cw`.
    pub fn shown(rect: Rect, rotation: i32) -> Rect {
        let mut r = rect;
        for _ in 0..rotation.rem_euclid(4) {
            r = Rect::new(1.0 - r.y - r.height, r.x, r.height, r.width);
        }
        r
    }

    /// The inverse of `shown`.
    pub fn unrotated(rect: Rect, rotation: i32) -> Rect {
        let mut r = rect;
        for _ in 0..rotation.rem_euclid(4) {
            r = Rect::new(r.y, 1.0 - r.x - r.width, r.height, r.width);
        }
        r
    }
}

/// An edge or corner: -1 is the left or top edge, 1 the right or bottom,
/// 0 neither. Both zero moves the rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Handle {
    pub x: i32,
    pub y: i32,
}

impl Handle {
    pub const MOVE: Handle = Handle { x: 0, y: 0 };
    pub fn corner(&self) -> bool {
        self.x != 0 && self.y != 0
    }
}

fn clamp(value: f64, low: f64, high: f64) -> f64 {
    value.max(low).min(low.max(high))
}

/// The largest `ratio` (width over height) rectangle in `bounds`, centred
/// on `center` as far as it fits.
pub fn fit(ratio: f64, center: Point, bounds: Rect) -> Rect {
    let width = bounds.width.min(bounds.height * ratio);
    let height = width / ratio;
    Rect::new(
        clamp(center.x - width / 2.0, bounds.x, bounds.max_x() - width),
        clamp(center.y - height / 2.0, bounds.y, bounds.max_y() - height),
        width,
        height,
    )
}

pub fn move_rect(rect: Rect, dx: f64, dy: f64, bounds: Rect) -> Rect {
    Rect::new(
        clamp(rect.x + dx, bounds.x, bounds.max_x() - rect.width),
        clamp(rect.y + dy, bounds.y, bounds.max_y() - rect.height),
        rect.width,
        rect.height,
    )
}

/// Drags `handle` of `rect` to `point`. The opposite edges stay put; with a
/// `ratio`, an edge drag grows the other side about its centre, and a corner
/// follows whichever side the pointer pulls further.
pub fn resize(rect: Rect, handle: Handle, point: Point, ratio: Option<f64>, bounds: Rect, minimum: f64) -> Rect {
    let p = Point { x: clamp(point.x, bounds.x, bounds.max_x()), y: clamp(point.y, bounds.y, bounds.max_y()) };
    let anchor = Point {
        x: if handle.x < 0 { rect.max_x() } else { rect.x },
        y: if handle.y < 0 { rect.max_y() } else { rect.y },
    };
    let room = Size::new(
        if handle.x > 0 { bounds.max_x() - anchor.x } else if handle.x < 0 { anchor.x - bounds.x } else { bounds.width },
        if handle.y > 0 { bounds.max_y() - anchor.y } else if handle.y < 0 { anchor.y - bounds.y } else { bounds.height },
    );
    let pulled = Size::new(
        (handle.x as f64 * (p.x - anchor.x)).max(0.0),
        (handle.y as f64 * (p.y - anchor.y)).max(0.0),
    );
    let mut size = rect.size();
    if let Some(ratio) = ratio {
        let mut width = if handle.x != 0 { pulled.width } else { 0.0 };
        if handle.y != 0 {
            width = width.max(pulled.height * ratio);
        }
        let smallest = minimum.max(minimum * ratio);
        width = width.max(smallest).min(room.width.min(room.height * ratio));
        size = Size::new(width, width / ratio);
    } else {
        if handle.x != 0 {
            size.width = pulled.width.max(minimum.min(room.width)).min(room.width);
        }
        if handle.y != 0 {
            size.height = pulled.height.max(minimum.min(room.height)).min(room.height);
        }
    }
    let origin = |side: i32, anchor: f64, length: f64, mid: f64, low: f64, high: f64| {
        if side > 0 {
            anchor
        } else if side < 0 {
            anchor - length
        } else {
            clamp(mid - length / 2.0, low, high - length)
        }
    };
    Rect::new(
        origin(handle.x, anchor.x, size.width, rect.mid_x(), bounds.x, bounds.max_x()),
        origin(handle.y, anchor.y, size.height, rect.mid_y(), bounds.y, bounds.max_y()),
        size.width,
        size.height,
    )
}

/// The handle under `point`, for a crop drawn at `frame` (both in view pixels).
/// `scale` is the view's pixels per DIP, so the reach feels the same at any DPI.
pub fn handle_at(point: Point, frame: Rect, scale: f64) -> Option<Handle> {
    let reach = (18.0 * scale).min(frame.width / 3.0).min(frame.height / 3.0);
    let edge = 8.0 * scale;
    let side = |value: f64, low: f64, high: f64, tolerance: f64| {
        if (value - low).abs() <= tolerance {
            Some(-1)
        } else if (value - high).abs() <= tolerance {
            Some(1)
        } else {
            None
        }
    };
    let outer = frame.inset(-edge, -edge);
    if !(point.x >= outer.x && point.x <= outer.max_x() && point.y >= outer.y && point.y <= outer.max_y()) {
        return None;
    }
    if let (Some(x), Some(y)) = (side(point.x, frame.x, frame.max_x(), reach), side(point.y, frame.y, frame.max_y(), reach)) {
        return Some(Handle { x, y });
    }
    if let Some(x) = side(point.x, frame.x, frame.max_x(), edge) {
        return Some(Handle { x, y: 0 });
    }
    if let Some(y) = side(point.y, frame.y, frame.max_y(), edge) {
        return Some(Handle { x: 0, y });
    }
    Some(Handle::MOVE)
}

/// `point` turned `degrees` clockwise (y down) about `center`.
pub fn turn(point: Point, degrees: f64, center: Point) -> Point {
    let (sin, cos) = (degrees * std::f64::consts::PI / 180.0).sin_cos();
    let (dx, dy) = (point.x - center.x, point.y - center.y);
    Point { x: center.x + dx * cos - dy * sin, y: center.y + dx * sin + dy * cos }
}

/// The corners of a `photo` turned by `angle`, clockwise from top left.
pub fn corners(photo: Size, angle: f64) -> [Point; 4] {
    let center = Point { x: photo.width / 2.0, y: photo.height / 2.0 };
    [
        Point { x: 0.0, y: 0.0 },
        Point { x: photo.width, y: 0.0 },
        Point { x: photo.width, y: photo.height },
        Point { x: 0.0, y: photo.height },
    ]
    .map(|p| turn(p, angle, center))
}

/// The turned photograph's bounding box.
pub fn bounds(photo: Size, angle: f64) -> Rect {
    let points = corners(photo, angle);
    let xs = points.map(|p| p.x);
    let ys = points.map(|p| p.y);
    let (x0, x1) = (xs.iter().cloned().fold(f64::INFINITY, f64::min), xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    let (y0, y1) = (ys.iter().cloned().fold(f64::INFINITY, f64::min), ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Whether `rect` lies on the photograph turned by `angle`.
pub fn fits(rect: Rect, angle: f64, photo: Size) -> bool {
    let center = Point { x: photo.width / 2.0, y: photo.height / 2.0 };
    let tolerance = 1e-6 * photo.width.max(photo.height).max(1.0);
    [
        Point { x: rect.x, y: rect.y },
        Point { x: rect.max_x(), y: rect.y },
        Point { x: rect.max_x(), y: rect.max_y() },
        Point { x: rect.x, y: rect.max_y() },
    ]
    .iter()
    .all(|&corner| {
        let p = turn(corner, -angle, center);
        p.x >= -tolerance && p.y >= -tolerance && p.x <= photo.width + tolerance && p.y <= photo.height + tolerance
    })
}

/// The furthest step from `start` towards `target` that stays on the turned photograph.
pub fn constrain(start: Rect, target: Rect, angle: f64, photo: Size) -> Rect {
    if fits(target, angle, photo) {
        return target;
    }
    if !fits(start, angle, photo) {
        return start;
    }
    let lerp = |t: f64| {
        Rect::new(
            start.x + (target.x - start.x) * t,
            start.y + (target.y - start.y) * t,
            start.width + (target.width - start.width) * t,
            start.height + (target.height - start.height) * t,
        )
    };
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..24 {
        let mid = (low + high) / 2.0;
        if fits(lerp(mid), angle, photo) {
            low = mid;
        } else {
            high = mid;
        }
    }
    lerp(low)
}

/// `rect` shrunk about its centre, never grown, until it lies on the
/// photograph turned by `angle`. A centre off the turned photograph moves
/// towards its centre, as little as keeps nearly the largest size.
pub fn inscribe(rect: Rect, angle: f64, photo: Size) -> Rect {
    let center = Point { x: photo.width / 2.0, y: photo.height / 2.0 };
    let origin = Point::default();
    let half = [
        Point { x: -rect.width / 2.0, y: -rect.height / 2.0 },
        Point { x: rect.width / 2.0, y: -rect.height / 2.0 },
        Point { x: rect.width / 2.0, y: rect.height / 2.0 },
        Point { x: -rect.width / 2.0, y: rect.height / 2.0 },
    ]
    .map(|p| turn(p, -angle, origin));
    let scale = |middle: Point| -> Option<f64> {
        let q = turn(middle, -angle, center);
        if !(q.x >= 0.0 && q.y >= 0.0 && q.x <= photo.width && q.y <= photo.height) {
            return None;
        }
        let mut s: f64 = 1.0;
        for v in &half {
            if v.x > 0.0 {
                s = s.min((photo.width - q.x) / v.x);
            } else if v.x < 0.0 {
                s = s.min(q.x / -v.x);
            }
            if v.y > 0.0 {
                s = s.min((photo.height - q.y) / v.y);
            } else if v.y < 0.0 {
                s = s.min(q.y / -v.y);
            }
        }
        Some(s.max(0.0))
    };
    let candidates: Vec<(Point, f64)> = (0..=20)
        .map(|i| {
            let t = i as f64 / 20.0;
            let middle = Point { x: rect.mid_x() + (center.x - rect.mid_x()) * t, y: rect.mid_y() + (center.y - rect.mid_y()) * t };
            (middle, scale(middle).unwrap_or(0.0))
        })
        .collect();
    let best = candidates.iter().map(|c| c.1).fold(0.0, f64::max);
    let Some(&(middle, s)) = candidates.iter().find(|c| c.1 >= best * 0.98).filter(|_| best > 0.0) else {
        return rect;
    };
    let size = Size::new(rect.width * s, rect.height * s);
    Rect::new(middle.x - size.width / 2.0, middle.y - size.height / 2.0, size.width, size.height)
}

/// The crop being drawn. Nothing changes the edit until Done.
#[derive(Clone, Debug)]
pub struct CropSession {
    /// A fraction of the unrotated photograph, like `CropSettings::rect`.
    pub rect: Rect,
    pub aspect: CropAspect,
    /// The locked ratio is taller than wide (for presets; custom uses its sides).
    pub portrait: bool,
    pub custom: Size,
    /// Degrees clockwise, as `CropSettings::angle`.
    pub angle: f64,
    /// The developed photograph as shown, in pixels, and its rotation.
    pub photo: Size,
    pub rotation: i32,
    /// A handle is being dragged; the thirds grid shows.
    pub adjusting: bool,
    /// The dial or slider is turning the photograph; a finer grid shows.
    pub straightening: bool,
    /// A ratio field has focus, so Return and Escape belong to it.
    pub typing: bool,
    /// The edit before cropping began; Cancel restores its rotation.
    pub start: crate::state::EditState,
    /// The crop when straightening began. The crop shrinks from it to stay on
    /// the turned photograph, and grows back as the angle returns.
    intended: Option<Rect>,
}

impl CropSession {
    pub fn new(start: crate::state::EditState, photo: Size, rotation: i32) -> Self {
        let settings = start.crop;
        let shown = CropSettings::shown(settings.rect, rotation);
        let portrait = shown.height * photo.height > shown.width * photo.width;
        let mut custom = settings.custom;
        if settings.aspect == CropAspect::Custom && (custom.height > custom.width) != portrait {
            custom = custom.swapped();
        }
        Self {
            rect: settings.rect,
            aspect: settings.aspect,
            portrait,
            custom,
            angle: settings.angle,
            photo,
            rotation,
            adjusting: false,
            straightening: false,
            typing: false,
            start,
            intended: None,
        }
    }

    pub fn settings(&self) -> CropSettings {
        if self.angle != 0.0 {
            return CropSettings { rect: self.rect, aspect: self.aspect, custom: self.custom, angle: self.angle };
        }
        let unit = self.rect.intersection(&Rect::UNIT);
        // Within a thousandth of the whole photograph is no crop.
        let whole = unit.x.abs() < 1e-3 && unit.y.abs() < 1e-3 && (unit.max_x() - 1.0).abs() < 1e-3 && (unit.max_y() - 1.0).abs() < 1e-3;
        CropSettings { rect: if whole { Rect::UNIT } else { unit }, aspect: self.aspect, custom: self.custom, angle: 0.0 }
    }

    pub fn modified(&self) -> bool {
        self.rect != Rect::UNIT || self.aspect != CropAspect::Original || self.angle != 0.0
    }

    pub fn orientation_portrait(&self) -> bool {
        if self.aspect == CropAspect::Custom { self.custom.height > self.custom.width } else { self.portrait }
    }

    /// Width over height of the locked ratio, as shown; None for freeform.
    pub fn ratio(&self) -> Option<f64> {
        if self.aspect == CropAspect::Custom {
            return (self.custom.width > 0.0 && self.custom.height > 0.0).then(|| self.custom.width / self.custom.height);
        }
        let ratio = self.aspect.ratio(self.photo)?;
        Some(if self.portrait { 1.0 / ratio } else { ratio })
    }

    /// The rectangle in pixels of the photograph as shown.
    pub fn shown(&self) -> Rect {
        let r = CropSettings::shown(self.rect, self.rotation);
        Rect::new(r.x * self.photo.width, r.y * self.photo.height, r.width * self.photo.width, r.height * self.photo.height)
    }

    pub fn set_shown(&mut self, value: Rect) {
        if self.photo.width <= 0.0 || self.photo.height <= 0.0 {
            return;
        }
        let r = Rect::new(value.x / self.photo.width, value.y / self.photo.height, value.width / self.photo.width, value.height / self.photo.height);
        self.rect = CropSettings::unrotated(r, self.rotation);
    }

    /// Where the rectangle can go: the turned photograph's bounding box.
    pub fn bounds(&self) -> Rect {
        bounds(self.photo, self.angle)
    }

    /// A newer render: a quarter turn swaps the locked ratio's orientation,
    /// as the rectangle turns with the photograph.
    pub fn update(&mut self, photo: Size, rotation: i32) {
        if (rotation - self.rotation) % 2 != 0 {
            self.portrait = !self.portrait;
            self.custom = self.custom.swapped();
        }
        self.photo = photo;
        self.rotation = rotation;
    }

    pub fn choose(&mut self, aspect: CropAspect) {
        self.aspect = aspect;
        if aspect == CropAspect::Original {
            self.portrait = self.photo.height > self.photo.width;
        }
        self.refit();
    }

    pub fn orient(&mut self, portrait: bool) {
        if self.aspect == CropAspect::Custom {
            if (self.custom.height > self.custom.width) != portrait {
                self.custom = self.custom.swapped();
            }
        } else {
            self.portrait = portrait;
        }
        self.refit();
    }

    pub fn set_custom(&mut self, width: Option<f64>, height: Option<f64>) {
        if let Some(width) = width {
            self.custom.width = width.clamp(0.1, 1000.0);
        }
        if let Some(height) = height {
            self.custom.height = height.clamp(0.1, 1000.0);
        }
        self.refit();
    }

    pub fn reset(&mut self) {
        self.rect = Rect::UNIT;
        self.aspect = CropAspect::Original;
        self.angle = 0.0;
        self.portrait = self.photo.height > self.photo.width;
    }

    pub fn begin_straightening(&mut self) {
        if self.intended.is_none() {
            self.intended = Some(self.shown());
        }
        self.straightening = true;
    }

    /// Turns the photograph, shrinking the crop as needed to stay on it.
    pub fn straighten(&mut self, degrees: f64) {
        let base = self.intended.unwrap_or_else(|| self.shown());
        self.angle = ((degrees * 10.0).round() / 10.0).clamp(-MAX_ANGLE, MAX_ANGLE);
        self.set_shown(inscribe(base, self.angle, self.photo));
    }

    pub fn end_straightening(&mut self) {
        self.intended = None;
        self.straightening = false;
    }

    /// The largest rectangle with the locked ratio, around the current centre.
    fn refit(&mut self) {
        let Some(ratio) = self.ratio() else { return };
        let current = self.shown();
        let fitted = fit(ratio, Point { x: current.mid_x(), y: current.mid_y() }, Rect::of_size(self.photo));
        self.set_shown(inscribe(fitted, self.angle, self.photo));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::EditState;

    fn assert_rect(a: Rect, b: Rect, accuracy: f64) {
        for (x, y) in [(a.x, b.x), (a.y, b.y), (a.width, b.width), (a.height, b.height)] {
            assert!((x - y).abs() <= accuracy, "{a:?} ≠ {b:?}");
        }
    }

    /// The same values as `pixels_round_edges_and_keep_one_pixel` in `spektrafilm-io::crop`.
    #[test]
    fn pixels_round_like_the_exporter() {
        let crop = CropSettings { rect: Rect::new(0.1004, 0.0, 0.5, 1.0), ..Default::default() };
        assert_eq!(crop.pixels(Size::new(1000.0, 800.0)), Rect::new(100.0, 0.0, 500.0, 800.0));
        let sliver = CropSettings { rect: Rect::new(0.999, 0.0, 0.0001, 1.0), ..Default::default() };
        assert_eq!(sliver.pixels(Size::new(10.0, 10.0)), Rect::new(9.0, 0.0, 1.0, 10.0));
        assert_eq!(CropSettings::default().size_of(Size::new(96.0, 64.0)), Size::new(96.0, 64.0));
    }

    /// Clockwise like `ImageBuf::rotated_cw`: the top-left corner turns to the top right.
    #[test]
    fn rectangle_turns_with_the_photograph() {
        let top_left = Rect::new(0.0, 0.0, 0.5, 0.25);
        assert_rect(CropSettings::shown(top_left, 1), Rect::new(0.75, 0.0, 0.25, 0.5), 1e-9);
        assert_rect(CropSettings::shown(top_left, 2), Rect::new(0.5, 0.75, 0.5, 0.25), 1e-9);
        let any = Rect::new(0.1, 0.2, 0.3, 0.4);
        for rotation in 0..4 {
            assert_rect(CropSettings::unrotated(CropSettings::shown(any, rotation), rotation), any, 1e-9);
        }
        assert_rect(CropSettings::shown(any, 4), any, 1e-9);
    }

    #[test]
    fn resize_keeps_the_ratio_and_stays_inside() {
        let bounds = Rect::of_size(Size::new(600.0, 400.0));
        let start = Rect::new(100.0, 100.0, 300.0, 200.0);
        let corner = resize(start, Handle { x: 1, y: 1 }, Point { x: 900.0, y: 900.0 }, Some(1.5), bounds, 10.0);
        assert_rect(corner, Rect::new(100.0, 100.0, 450.0, 300.0), 1e-9);
        let edge = resize(start, Handle { x: 1, y: 0 }, Point { x: 460.0, y: 0.0 }, Some(1.5), bounds, 10.0);
        assert_rect(edge, Rect::new(100.0, 80.0, 360.0, 240.0), 1e-9);
        let free = resize(start, Handle { x: -1, y: -1 }, Point { x: 395.0, y: -50.0 }, None, bounds, 20.0);
        assert_rect(free, Rect::new(380.0, 0.0, 20.0, 300.0), 1e-9);
        let moved = move_rect(start, 1000.0, -1000.0, bounds);
        assert_rect(moved, Rect::new(300.0, 0.0, 300.0, 200.0), 1e-9);
        assert_rect(fit(1.0, Point { x: 580.0, y: 200.0 }, bounds), Rect::new(200.0, 0.0, 400.0, 400.0), 1e-9);
    }

    #[test]
    fn handles_under_the_pointer() {
        let frame = Rect::new(100.0, 100.0, 300.0, 200.0);
        let at = |x, y| handle_at(Point { x, y }, frame, 1.0);
        assert_eq!(at(104.0, 96.0), Some(Handle { x: -1, y: -1 }));
        assert_eq!(at(398.0, 200.0), Some(Handle { x: 1, y: 0 }));
        assert_eq!(at(250.0, 302.0), Some(Handle { x: 0, y: 1 }));
        assert_eq!(at(250.0, 200.0), Some(Handle::MOVE));
        assert_eq!(at(20.0, 20.0), None);
    }

    #[test]
    fn session_locks_ratios_and_turns_them_with_the_photograph() {
        let mut session = CropSession::new(EditState::default(), Size::new(600.0, 400.0), 0);
        assert!(!session.modified());
        session.choose(CropAspect::Square);
        assert_rect(session.shown(), Rect::new(100.0, 0.0, 400.0, 400.0), 1e-9);
        session.choose(CropAspect::ThreeTwo);
        session.orient(true);
        assert_rect(session.shown(), Rect::new(166.666666, 0.0, 266.666666, 400.0), 1e-3);
        let rect = session.rect;
        session.update(Size::new(400.0, 600.0), 1);
        assert_eq!(session.rect, rect);
        assert!(!session.orientation_portrait());
        assert!((session.ratio().unwrap() - 1.5).abs() < 1e-9);
        session.choose(CropAspect::Custom);
        session.set_custom(Some(4.0), Some(5.0));
        assert!(session.orientation_portrait());
        assert!((session.shown().width / session.shown().height - 0.8).abs() < 1e-9);
        session.reset();
        assert_eq!(
            session.settings(),
            CropSettings { rect: Rect::UNIT, aspect: CropAspect::Original, custom: Size::new(4.0, 5.0), angle: 0.0 }
        );
        assert!(!session.modified());
    }

    #[test]
    fn original_is_the_default_aspect() {
        assert_eq!(CropSettings::default().aspect, CropAspect::Original);
        let session = CropSession::new(EditState::default(), Size::new(400.0, 600.0), 0);
        assert!((session.ratio().unwrap() - 2.0 / 3.0).abs() < 1e-9);
        let resized = resize(session.shown(), Handle { x: 1, y: 1 }, Point { x: 300.0, y: 100.0 }, session.ratio(), Rect::of_size(session.photo), 10.0);
        assert!((resized.width / resized.height - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn straightening_shrinks_the_crop_onto_the_turned_photograph() {
        let photo = Size::new(600.0, 400.0);
        let mut session = CropSession::new(EditState::default(), photo, 0);
        session.begin_straightening();
        session.straighten(5.0);
        assert_eq!(session.angle, 5.0);
        let shown = session.shown();
        assert!(fits(shown, 5.0, photo));
        assert!((shown.width / shown.height - 1.5).abs() < 1e-6);
        assert!((shown.mid_x() - 300.0).abs() < 1e-6 && (shown.mid_y() - 200.0).abs() < 1e-6);
        assert!(shown.width < 600.0);
        let larger = shown.inset(-shown.width * 0.005, -shown.height * 0.005);
        assert!(!fits(larger, 5.0, photo));
        session.straighten(0.0);
        assert_rect(session.shown(), Rect::of_size(photo), 1e-6);
        session.straighten(-3.04);
        session.end_straightening();
        assert_eq!(session.angle, -3.0);
        assert_eq!(session.settings().angle, -3.0);
        assert!(session.settings().cropped());
        assert!(session.modified());
        session.straighten(99.0);
        assert_eq!(session.angle, 45.0);
    }

    #[test]
    fn constrain_stops_at_the_turned_edge() {
        let photo = Size::new(600.0, 400.0);
        let start = inscribe(Rect::new(250.0, 150.0, 100.0, 100.0), 10.0, photo);
        assert_eq!(start, Rect::new(250.0, 150.0, 100.0, 100.0));
        let far = start.offset(1000.0, 0.0);
        let stopped = constrain(start, far, 10.0, photo);
        assert!(fits(stopped, 10.0, photo));
        assert!(stopped.x > start.x);
        assert!(!fits(stopped.offset(1.0, 0.0), 10.0, photo));
        let b = bounds(photo, 10.0);
        assert!(b.y < 0.0 && b.max_x() > 600.0);
    }

    #[test]
    fn straightened_size_rounds_sides_like_the_exporter() {
        let crop = CropSettings { rect: Rect::new(0.2, 0.1, 0.5, 0.25), angle: -3.0, ..Default::default() };
        assert_eq!(crop.size_of(Size::new(1000.0, 800.0)), Size::new(500.0, 200.0));
    }

    #[test]
    fn crop_edits_do_not_change_the_rendered_state() {
        let mut state = EditState::default();
        let rendered = state.rendered();
        state.crop = CropSettings { rect: Rect::new(0.0, 0.0, 0.5, 0.5), ..Default::default() };
        assert_eq!(state.rendered(), rendered);
    }

    #[test]
    fn settings_round_trip_the_mac_session_format() {
        let json = r#"{"rect":[[0.1,0.2],[0.5,0.25]],"aspect":"3:2","custom":[5,4],"angle":-3}"#;
        let crop: CropSettings = serde_json::from_str(json).unwrap();
        assert_eq!(crop.aspect, CropAspect::ThreeTwo);
        assert_eq!(crop.rect, Rect::new(0.1, 0.2, 0.5, 0.25));
        let again: CropSettings = serde_json::from_str(&serde_json::to_string(&crop).unwrap()).unwrap();
        assert_eq!(again, crop);
    }
}
