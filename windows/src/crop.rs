//! A presentation crop, like the border: changing one never re-renders. The
//! rectangle is a fraction of the unrotated photograph, origin top left, so
//! it turns with the photograph. Straightening turns the photograph by
//! `angle` degrees (clockwise as shown) about its centre, and the rectangle
//! is then in that turned frame. Export and Save Preview crop with
//! `spektrafilm-io::crop`, which this module calls for the pixel rounding.
//! Mirrors `Crop.swift` in the macOS app.
//!
//! The shared core's (core/src/rules/crop, through negs_core_rules): the
//! types and their serde stay Rust, the geometry runs in Odin
//! (core/diff/tests/crop.rs). The crop session stays here, as it holds the
//! edit state; its geometry is the core's `SessionGeometry`.

pub use negs_core_rules::crop::*;

/// The crop being drawn. Nothing changes the edit until Done.
#[derive(Clone, Debug)]
pub struct CropSession {
    /// A fraction of the unrotated photograph, like `CropSettings::rect`.
    pub rect: Rect,
    pub aspect: CropAspect,
    /// The locked ratio is taller than wide (for presets; custom uses its sides).
    pub portrait: bool,
    pub custom: Size,
    /// Degrees clockwise as shown: `CropSettings::angle`, the other way
    /// round on a flipped photograph.
    pub angle: f64,
    /// The developed photograph as shown, in pixels, its rotation and flip.
    pub photo: Size,
    pub rotation: i32,
    pub flip: bool,
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
    pub fn new(start: crate::state::EditState, photo: Size, rotation: i32, flip: bool) -> Self {
        let g = SessionGeometry::new(&start.crop, photo, rotation, flip);
        Self {
            rect: g.rect,
            aspect: g.aspect,
            portrait: g.portrait,
            custom: g.custom,
            angle: g.angle,
            photo: g.photo,
            rotation: g.rotation,
            flip: g.flip,
            adjusting: false,
            straightening: g.straightening,
            typing: false,
            start,
            intended: g.intended,
        }
    }

    /// The session's geometry, for the core.
    fn geometry(&self) -> SessionGeometry {
        SessionGeometry {
            rect: self.rect,
            aspect: self.aspect,
            portrait: self.portrait,
            custom: self.custom,
            angle: self.angle,
            photo: self.photo,
            rotation: self.rotation,
            flip: self.flip,
            straightening: self.straightening,
            intended: self.intended,
        }
    }

    /// Changes the geometry with `f`, on the core.
    fn change(&mut self, f: impl FnOnce(&mut SessionGeometry)) {
        let mut g = self.geometry();
        f(&mut g);
        self.rect = g.rect;
        self.aspect = g.aspect;
        self.portrait = g.portrait;
        self.custom = g.custom;
        self.angle = g.angle;
        self.photo = g.photo;
        self.rotation = g.rotation;
        self.flip = g.flip;
        self.straightening = g.straightening;
        self.intended = g.intended;
    }

    pub fn settings(&self) -> CropSettings {
        self.geometry().settings()
    }

    pub fn modified(&self) -> bool {
        self.geometry().modified()
    }

    pub fn orientation_portrait(&self) -> bool {
        self.geometry().orientation_portrait()
    }

    /// Width over height of the locked ratio, as shown; None for freeform.
    pub fn ratio(&self) -> Option<f64> {
        self.geometry().ratio()
    }

    /// The rectangle in pixels of the photograph as shown.
    pub fn shown(&self) -> Rect {
        self.geometry().shown()
    }

    pub fn set_shown(&mut self, value: Rect) {
        self.change(|g| g.set_shown(value));
    }

    /// Where the rectangle can go: the turned photograph's bounding box.
    pub fn bounds(&self) -> Rect {
        self.geometry().bounds()
    }

    /// A newer render: a quarter turn swaps the locked ratio's orientation,
    /// as the rectangle turns with the photograph. A flip mirrors the
    /// rectangle and the straightening with it.
    pub fn update(&mut self, photo: Size, rotation: i32, flip: bool) {
        self.change(|g| g.update(photo, rotation, flip));
    }

    pub fn choose(&mut self, aspect: CropAspect) {
        self.change(|g| g.choose(aspect));
    }

    pub fn orient(&mut self, portrait: bool) {
        self.change(|g| g.orient(portrait));
    }

    pub fn set_custom(&mut self, width: Option<f64>, height: Option<f64>) {
        self.change(|g| g.set_custom(width, height));
    }

    pub fn reset(&mut self) {
        self.change(|g| g.reset());
    }

    pub fn begin_straightening(&mut self) {
        self.change(|g| g.begin_straightening());
    }

    /// Turns the photograph, shrinking the crop as needed to stay on it.
    pub fn straighten(&mut self, degrees: f64) {
        self.change(|g| g.straighten(degrees));
    }

    pub fn end_straightening(&mut self) {
        self.change(|g| g.end_straightening());
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
        assert_rect(CropSettings::shown(top_left, 1, false), Rect::new(0.75, 0.0, 0.25, 0.5), 1e-9);
        assert_rect(CropSettings::shown(top_left, 2, false), Rect::new(0.5, 0.75, 0.5, 0.25), 1e-9);
        let any = Rect::new(0.1, 0.2, 0.3, 0.4);
        for rotation in 0..4 {
            assert_rect(CropSettings::unrotated(CropSettings::shown(any, rotation, false), rotation, false), any, 1e-9);
        }
        assert_rect(CropSettings::shown(any, 4, false), any, 1e-9);
        // Mirrored first, then turned, like `ImageBuf::flipped_rotated_cw`.
        assert_rect(CropSettings::shown(top_left, 0, true), Rect::new(0.5, 0.0, 0.5, 0.25), 1e-9);
        assert_rect(CropSettings::shown(top_left, 1, true), Rect::new(0.75, 0.5, 0.25, 0.5), 1e-9);
        for rotation in 0..4 {
            assert_rect(CropSettings::unrotated(CropSettings::shown(any, rotation, true), rotation, true), any, 1e-9);
        }
    }

    /// Flip mirrors as shown (`Orientation.flippedAsShown`): a turned
    /// photograph's turns are conjugated, so flipping twice is where it began.
    #[test]
    fn flipping_mirrors_the_photograph_as_shown() {
        for turns in 0..4 {
            for flip in [false, true] {
                let o = Orientation::new(turns, flip);
                let f = o.flipped_as_shown();
                assert_eq!(f.flipped_as_shown(), o);
                for p in [Point { x: 0.2, y: 0.7 }, Point { x: 0.9, y: 0.1 }] {
                    let (a, b) = (o.shown(p), f.shown(p));
                    assert!((a.x - (1.0 - b.x)).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9, "{o:?} {p:?}");
                    let back = o.unshown(a);
                    assert!((back.x - p.x).abs() < 1e-9 && (back.y - p.y).abs() < 1e-9);
                }
            }
        }
    }

    /// The straightening is kept in the unflipped photograph: the session
    /// shows it the other way round on a flipped one.
    #[test]
    fn a_flip_mirrors_the_straightening() {
        let mut start = EditState::default();
        start.crop.angle = 3.0;
        start.crop.rect = Rect::new(0.1, 0.1, 0.5, 0.5);
        let mut session = CropSession::new(start.clone(), Size::new(600.0, 400.0), 0, true);
        assert_eq!(session.angle, -3.0);
        assert_eq!(session.settings().angle, 3.0);
        session.update(Size::new(600.0, 400.0), 0, false);
        assert_eq!((session.angle, session.settings().angle), (3.0, 3.0));
        let shown = CropSession::new(start, Size::new(600.0, 400.0), 0, true).shown();
        assert_rect(shown, Rect::new(240.0, 40.0, 300.0, 200.0), 1e-9);
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
        let mut session = CropSession::new(EditState::default(), Size::new(600.0, 400.0), 0, false);
        assert!(!session.modified());
        session.choose(CropAspect::Square);
        assert_rect(session.shown(), Rect::new(100.0, 0.0, 400.0, 400.0), 1e-9);
        session.choose(CropAspect::ThreeTwo);
        session.orient(true);
        assert_rect(session.shown(), Rect::new(166.666666, 0.0, 266.666666, 400.0), 1e-3);
        let rect = session.rect;
        session.update(Size::new(400.0, 600.0), 1, false);
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
        let session = CropSession::new(EditState::default(), Size::new(400.0, 600.0), 0, false);
        assert!((session.ratio().unwrap() - 2.0 / 3.0).abs() < 1e-9);
        let resized = resize(session.shown(), Handle { x: 1, y: 1 }, Point { x: 300.0, y: 100.0 }, session.ratio(), Rect::of_size(session.photo), 10.0);
        assert!((resized.width / resized.height - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn straightening_shrinks_the_crop_onto_the_turned_photograph() {
        let photo = Size::new(600.0, 400.0);
        let mut session = CropSession::new(EditState::default(), photo, 0, false);
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
