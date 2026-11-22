//! windows/src/crop.rs's API with the geometry in the shared Odin core
//! (core/src/rules/crop). A presentation crop, like the border: changing
//! one never re-renders. The rectangle is a fraction of the unrotated
//! photograph, origin top left, so it turns with the photograph.
//! Straightening turns the photograph by `angle` degrees (clockwise as
//! shown) about its centre, and the rectangle is then in that turned frame.
//!
//! What runs where:
//! - In Odin: every rule (the exporter's pixel rounding, the turns and
//!   flips, fitting, dragging, the handles, straightening, the session's
//!   geometry), the aspects' words and ratios.
//! - In Rust: the plain types with their serde formats (the session file's
//!   and the sidecars'), their field accessors, and `CropSettings::engine`,
//!   which builds the engine's own `Crop`. `CropSession` stays in
//!   windows/src/crop.rs (it holds the edit state, Phase 4's) and keeps its
//!   geometry in a [`SessionGeometry`].

use negs_core_sys::picture as sys;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::os::raw::c_char;

pub const MAX_ANGLE: f64 = spektrafilm_io::crop::MAX_ANGLE;

/// A static string from the core.
pub(crate) fn static_str(p: *const c_char) -> &'static str {
    if p.is_null() {
        return "";
    }
    unsafe { std::ffi::CStr::from_ptr(p) }.to_str().unwrap_or("")
}

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
        let mut out = sys::negs_rect_t::default();
        unsafe { sys::negs_rect_inset(c_rect(*self), dx, dy, &mut out) };
        rect(out)
    }
    pub fn contains(&self, p: Point) -> bool {
        unsafe { sys::negs_rect_contains(c_rect(*self), c_pt(p)) != 0 }
    }
    pub fn intersection(&self, other: &Rect) -> Rect {
        let mut out = sys::negs_rect_t::default();
        unsafe { sys::negs_rect_intersection(c_rect(*self), c_rect(*other), &mut out) };
        rect(out)
    }
    /// The rectangle moved (windows/src/crop.rs's tests use it).
    pub fn offset(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }
}

pub(crate) fn c_rect(r: Rect) -> sys::negs_rect_t {
    sys::negs_rect_t { x: r.x, y: r.y, w: r.width, h: r.height }
}
pub(crate) fn rect(r: sys::negs_rect_t) -> Rect {
    Rect::new(r.x, r.y, r.w, r.h)
}
pub(crate) fn c_pt(p: Point) -> sys::negs_pt_t {
    sys::negs_pt_t { x: p.x, y: p.y }
}
pub(crate) fn pt(p: sys::negs_pt_t) -> Point {
    Point { x: p.x, y: p.y }
}
pub(crate) fn c_size(s: Size) -> sys::negs_size_t {
    sys::negs_size_t { width: s.width, height: s.height }
}
pub(crate) fn size(s: sys::negs_size_t) -> Size {
    Size::new(s.width, s.height)
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

/// The cases are in the core's order (`NEGS_CROP_*`).
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

    pub(crate) fn id(self) -> u32 {
        self as u32
    }

    pub(crate) fn from_id(id: u32) -> Self {
        Self::ALL.get(id as usize).copied().unwrap_or_default()
    }

    pub fn title(self) -> &'static str {
        static_str(unsafe { sys::negs_crop_aspect_title(self.id()) })
    }

    /// The film or print format with that shape.
    pub fn format(self) -> Option<&'static str> {
        let p = unsafe { sys::negs_crop_aspect_format(self.id()) };
        (!p.is_null()).then(|| static_str(p))
    }

    /// The long side over the short side, for a photograph shown at `photo`.
    pub fn ratio(self, photo: Size) -> Option<f64> {
        let mut out = 0.0;
        (unsafe { sys::negs_crop_aspect_ratio(self.id(), c_size(photo), &mut out) } != 0).then_some(out)
    }

    /// Whether the ratio has a landscape and a portrait orientation.
    pub fn turns(self) -> bool {
        unsafe { sys::negs_crop_aspect_turns(self.id()) != 0 }
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
    pub(crate) fn c(&self) -> sys::negs_crop_settings_t {
        sys::negs_crop_settings_t { rect: c_rect(self.rect), custom: c_size(self.custom), angle: self.angle, aspect: self.aspect.id(), reserved: 0 }
    }

    pub(crate) fn of(c: &sys::negs_crop_settings_t) -> Self {
        Self { rect: rect(c.rect), aspect: CropAspect::from_id(c.aspect), custom: size(c.custom), angle: c.angle }
    }

    pub fn cropped(&self) -> bool {
        unsafe { sys::negs_crop_cropped(&self.c()) != 0 }
    }

    pub fn engine(&self) -> spektrafilm_io::crop::Crop {
        spektrafilm_io::crop::Crop { x: self.rect.x, y: self.rect.y, width: self.rect.width, height: self.rect.height, angle: self.angle }
    }

    /// The pixel rectangle in a `size` photograph, rounded like the
    /// exporter: edges round to the nearest pixel, and at least one pixel
    /// remains. A straightened crop keeps its origin and rounds its sides.
    pub fn pixels(&self, size: Size) -> Rect {
        let mut out = sys::negs_rect_t::default();
        unsafe { sys::negs_crop_pixels(&self.c(), c_size(size), &mut out) };
        rect(out)
    }

    /// The cropped photograph's size, before rotation.
    pub fn size_of(&self, size: Size) -> Size {
        let mut out = sys::negs_size_t::default();
        unsafe { sys::negs_crop_size_of(&self.c(), c_size(size), &mut out) };
        self::size(out)
    }

    /// The rectangle as the photograph is shown: mirrored left to right if
    /// `flip`, then after `rotation` clockwise quarter turns, like
    /// `ImageBuf::flipped_rotated_cw`.
    pub fn shown(rect: Rect, rotation: i32, flip: bool) -> Rect {
        let mut out = sys::negs_rect_t::default();
        unsafe { sys::negs_crop_shown(c_rect(rect), rotation, flip as u8, &mut out) };
        self::rect(out)
    }

    /// The inverse of `shown`.
    pub fn unrotated(rect: Rect, rotation: i32, flip: bool) -> Rect {
        let mut out = sys::negs_rect_t::default();
        unsafe { sys::negs_crop_unrotated(c_rect(rect), rotation, flip as u8, &mut out) };
        self::rect(out)
    }

    /// A fraction of the photograph mirrored left to right.
    pub fn mirrored(rect: Rect) -> Rect {
        let mut out = sys::negs_rect_t::default();
        unsafe { sys::negs_crop_mirrored(c_rect(rect), &mut out) };
        self::rect(out)
    }
}

/// How the cropped photograph is shown (`io.flip`, `io.rotate_cw`): mirrored
/// left to right if `flip`, then turned `turns` quarter turns clockwise. A
/// mirror top to bottom is a flip and a half turn, so one Flip covers both
/// (`Orientation` in `Crop.swift`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Orientation {
    pub turns: i32,
    pub flip: bool,
}

impl Orientation {
    pub(crate) fn c(self) -> sys::negs_orientation_t {
        sys::negs_orientation_t { turns: self.turns, flip: self.flip as u8, reserved: [0; 3] }
    }

    fn of(c: sys::negs_orientation_t) -> Self {
        Self { turns: c.turns, flip: c.flip != 0 }
    }

    pub fn new(turns: i32, flip: bool) -> Self {
        let mut out = sys::negs_orientation_t::default();
        unsafe { sys::negs_orientation_new(turns, flip as u8, &mut out) };
        Self::of(out)
    }

    /// Mirrored left to right as shown: a flip conjugates the turns, so a
    /// quarter turn becomes three.
    pub fn flipped_as_shown(self) -> Self {
        let mut out = sys::negs_orientation_t::default();
        unsafe { sys::negs_orientation_flipped_as_shown(self.c(), &mut out) };
        Self::of(out)
    }

    /// Where a point at `fraction` of the cropped photograph (unmirrored,
    /// unturned, from its top left) is shown, as a fraction of the shown photograph.
    pub fn shown(self, fraction: Point) -> Point {
        let mut out = sys::negs_pt_t::default();
        unsafe { sys::negs_orientation_shown(self.c(), c_pt(fraction), &mut out) };
        pt(out)
    }

    /// The inverse of `shown`.
    pub fn unshown(self, fraction: Point) -> Point {
        let mut out = sys::negs_pt_t::default();
        unsafe { sys::negs_orientation_unshown(self.c(), c_pt(fraction), &mut out) };
        pt(out)
    }
}

/// A point at `shown` (fractions of the photograph as the canvas shows it
/// through `crop`) back to a fraction of the whole, uncropped photograph:
/// through the crop, its straightening (turned back about the photograph's
/// centre, as the canvas's shader samples it), the turns and the flip.
/// `turned` is the whole photograph's size as shown, after the turns. None
/// outside the photograph.
pub fn unshown_point(orientation: Orientation, crop: &CropSettings, turned: Size, shown: [f64; 2]) -> Option<Point> {
    let mut out = sys::negs_pt_t::default();
    (unsafe { sys::negs_crop_unshown_point(orientation.c(), &crop.c(), c_size(turned), shown[0], shown[1], &mut out) } != 0).then(|| pt(out))
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
}

fn rect_call(f: impl FnOnce(*mut sys::negs_rect_t)) -> Rect {
    let mut out = sys::negs_rect_t::default();
    f(&mut out);
    rect(out)
}

/// The largest `ratio` (width over height) rectangle in `bounds`, centred
/// on `center` as far as it fits.
pub fn fit(ratio: f64, center: Point, bounds: Rect) -> Rect {
    rect_call(|out| unsafe { sys::negs_crop_fit(ratio, c_pt(center), c_rect(bounds), out) })
}

pub fn move_rect(rect: Rect, dx: f64, dy: f64, bounds: Rect) -> Rect {
    rect_call(|out| unsafe { sys::negs_crop_move_rect(c_rect(rect), dx, dy, c_rect(bounds), out) })
}

/// Drags `handle` of `rect` to `point`. The opposite edges stay put; with a
/// `ratio`, an edge drag grows the other side about its centre, and a corner
/// follows whichever side the pointer pulls further.
pub fn resize(rect: Rect, handle: Handle, point: Point, ratio: Option<f64>, bounds: Rect, minimum: f64) -> Rect {
    let h = sys::negs_crop_handle_t { x: handle.x, y: handle.y };
    rect_call(|out| unsafe {
        sys::negs_crop_resize(c_rect(rect), h, c_pt(point), ratio.is_some() as u8, ratio.unwrap_or(0.0), c_rect(bounds), minimum, out)
    })
}

/// The handle under `point`, for a crop drawn at `frame` (both in view pixels).
/// `scale` is the view's pixels per DIP, so the reach feels the same at any DPI.
pub fn handle_at(point: Point, frame: Rect, scale: f64) -> Option<Handle> {
    let mut out = sys::negs_crop_handle_t::default();
    (unsafe { sys::negs_crop_handle_at(c_pt(point), c_rect(frame), scale, &mut out) } != 0).then_some(Handle { x: out.x, y: out.y })
}

/// `point` turned `degrees` clockwise (y down) about `center`.
pub fn turn(point: Point, degrees: f64, center: Point) -> Point {
    let mut out = sys::negs_pt_t::default();
    unsafe { sys::negs_crop_turn(c_pt(point), degrees, c_pt(center), &mut out) };
    pt(out)
}

/// The corners of a `photo` turned by `angle`, clockwise from top left.
pub fn corners(photo: Size, angle: f64) -> [Point; 4] {
    let mut out = [sys::negs_pt_t::default(); 4];
    unsafe { sys::negs_crop_corners(c_size(photo), angle, &mut out) };
    out.map(pt)
}

/// The turned photograph's bounding box.
pub fn bounds(photo: Size, angle: f64) -> Rect {
    rect_call(|out| unsafe { sys::negs_crop_bounds(c_size(photo), angle, out) })
}

/// Whether `rect` lies on the photograph turned by `angle`.
pub fn fits(rect: Rect, angle: f64, photo: Size) -> bool {
    unsafe { sys::negs_crop_fits(c_rect(rect), angle, c_size(photo)) != 0 }
}

/// The furthest step from `start` towards `target` that stays on the turned photograph.
pub fn constrain(start: Rect, target: Rect, angle: f64, photo: Size) -> Rect {
    rect_call(|out| unsafe { sys::negs_crop_constrain(c_rect(start), c_rect(target), angle, c_size(photo), out) })
}

/// `rect` shrunk about its centre, never grown, until it lies on the
/// photograph turned by `angle`. A centre off the turned photograph moves
/// towards its centre, as little as keeps nearly the largest size.
pub fn inscribe(rect: Rect, angle: f64, photo: Size) -> Rect {
    rect_call(|out| unsafe { sys::negs_crop_inscribe(c_rect(rect), angle, c_size(photo), out) })
}

/// The geometry of the crop being drawn (`CropSession` without the edit it
/// began from and the pointer's flags): the session calls run in the core.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionGeometry {
    pub rect: Rect,
    pub aspect: CropAspect,
    pub portrait: bool,
    pub custom: Size,
    pub angle: f64,
    pub photo: Size,
    pub rotation: i32,
    pub flip: bool,
    pub straightening: bool,
    pub intended: Option<Rect>,
}

impl SessionGeometry {
    fn c(&self) -> sys::negs_crop_session_t {
        sys::negs_crop_session_t {
            rect: c_rect(self.rect),
            custom: c_size(self.custom),
            photo: c_size(self.photo),
            intended: c_rect(self.intended.unwrap_or_default()),
            angle: self.angle,
            rotation: self.rotation,
            aspect: self.aspect.id(),
            portrait: self.portrait as u8,
            flip: self.flip as u8,
            straightening: self.straightening as u8,
            has_intended: self.intended.is_some() as u8,
            reserved: 0,
        }
    }

    fn of(c: &sys::negs_crop_session_t) -> Self {
        Self {
            rect: rect(c.rect),
            aspect: CropAspect::from_id(c.aspect),
            portrait: c.portrait != 0,
            custom: size(c.custom),
            angle: c.angle,
            photo: size(c.photo),
            rotation: c.rotation,
            flip: c.flip != 0,
            straightening: c.straightening != 0,
            intended: (c.has_intended != 0).then(|| rect(c.intended)),
        }
    }

    /// A call that changes the session.
    fn change(&mut self, f: impl FnOnce(*mut sys::negs_crop_session_t)) {
        let mut c = self.c();
        f(&mut c);
        *self = Self::of(&c);
    }

    pub fn new(start: &CropSettings, photo: Size, rotation: i32, flip: bool) -> Self {
        let mut out = sys::negs_crop_session_t::default();
        unsafe { sys::negs_crop_session_new(&start.c(), c_size(photo), rotation, flip as u8, &mut out) };
        Self::of(&out)
    }

    pub fn settings(&self) -> CropSettings {
        let mut out = sys::negs_crop_settings_t::default();
        unsafe { sys::negs_crop_session_settings(&self.c(), &mut out) };
        CropSettings::of(&out)
    }

    pub fn modified(&self) -> bool {
        unsafe { sys::negs_crop_session_modified(&self.c()) != 0 }
    }

    pub fn orientation_portrait(&self) -> bool {
        unsafe { sys::negs_crop_session_orientation_portrait(&self.c()) != 0 }
    }

    pub fn ratio(&self) -> Option<f64> {
        let mut out = 0.0;
        (unsafe { sys::negs_crop_session_ratio(&self.c(), &mut out) } != 0).then_some(out)
    }

    pub fn shown(&self) -> Rect {
        rect_call(|out| unsafe { sys::negs_crop_session_shown(&self.c(), out) })
    }

    pub fn set_shown(&mut self, value: Rect) {
        self.change(|s| unsafe { sys::negs_crop_session_set_shown(s, c_rect(value)) });
    }

    pub fn bounds(&self) -> Rect {
        rect_call(|out| unsafe { sys::negs_crop_session_bounds(&self.c(), out) })
    }

    pub fn update(&mut self, photo: Size, rotation: i32, flip: bool) {
        self.change(|s| unsafe { sys::negs_crop_session_update(s, c_size(photo), rotation, flip as u8) });
    }

    pub fn choose(&mut self, aspect: CropAspect) {
        self.change(|s| unsafe { sys::negs_crop_session_choose(s, aspect.id()) });
    }

    pub fn orient(&mut self, portrait: bool) {
        self.change(|s| unsafe { sys::negs_crop_session_orient(s, portrait as u8) });
    }

    pub fn set_custom(&mut self, width: Option<f64>, height: Option<f64>) {
        self.change(|s| unsafe {
            sys::negs_crop_session_set_custom(s, width.is_some() as u8, width.unwrap_or(0.0), height.is_some() as u8, height.unwrap_or(0.0))
        });
    }

    pub fn reset(&mut self) {
        self.change(|s| unsafe { sys::negs_crop_session_reset(s) });
    }

    pub fn begin_straightening(&mut self) {
        self.change(|s| unsafe { sys::negs_crop_session_begin_straightening(s) });
    }

    pub fn straighten(&mut self, degrees: f64) {
        self.change(|s| unsafe { sys::negs_crop_session_straighten(s, degrees) });
    }

    pub fn end_straightening(&mut self) {
        self.change(|s| unsafe { sys::negs_crop_session_end_straightening(s) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_angle_is_the_cores_and_the_engines() {
        assert_eq!(MAX_ANGLE, unsafe { sys::negs_crop_max_angle() });
    }

    #[test]
    fn aspects_words_come_from_the_core() {
        let titles: Vec<&str> = CropAspect::ALL.iter().map(|a| a.title()).collect();
        assert_eq!(titles, ["Original", "Freeform", "Square", "16:9", "3:2", "4:3", "5:4", "7:5", "Custom"]);
        assert_eq!(CropAspect::FiveFour.format(), Some("4×5"));
        assert_eq!(CropAspect::Custom.format(), None);
        assert_eq!(CropAspect::SixteenNine.ratio(Size::ZERO), Some(16.0 / 9.0));
        assert_eq!(CropAspect::Original.ratio(Size::new(400.0, 600.0)), Some(1.5));
        assert!(!CropAspect::Square.turns() && CropAspect::ThreeTwo.turns());
    }
}
