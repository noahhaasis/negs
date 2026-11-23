//! windows/src/state.rs's edit state on the shared core (core/src/state):
//! EditState, BrightnessMatch and Roll stay Rust structs with their derives
//! (sidecars and sessions keep serde's own text); their rules run in Odin.
//! A whole state crosses as `serde_json::to_vec(&state)` text into a
//! `negs_state_t` handle and comes back the same way; rules that read one
//! value (rotation, flip, the size target, a roll's shifts) send only that
//! value's JSON.
//!
//! Kept in Rust: `EditState::value` (a borrow into `params`), the path
//! constants (`const`s, checked against the core's in the tests), and
//! `resolve_size` (it asks the engine).
//!
//! Phase 4a adds the rest of state.rs (stocks, filters, groups) below.

use negs_core_sys::state as sys;
use negs_core_sys::take_buf;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::border::BorderSettings;
use crate::crop::{CropSettings, static_str};
use crate::json;

// MARK: Phase 4 base

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditState {
    pub params: Value,
    pub film: String,
    pub paper: String,
    #[serde(default)]
    pub limit_preview: bool,
    /// Drawn around the developed photograph; not part of the render.
    #[serde(default)]
    pub border: BorderSettings,
    /// Trims the developed photograph; not part of the render.
    #[serde(default)]
    pub crop: CropSettings,
    /// Prep › Match camera brightness: present while it's on. Sessions from
    /// before it read as off and keep their exposure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness_match: Option<BrightnessMatch>,
    /// A scanned negative printed as a frame of a darkroom roll (§9.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roll: Option<Roll>,
}

/// A darkroom roll (§9.2, `NegativeRoll`): frames printed alike. See
/// windows/src/state.rs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Roll {
    pub id: String,
    /// The roll's enlarger filtration: magenta and yellow shifts.
    pub magenta: f64,
    pub yellow: f64,
    /// The paper the roll's filtration is balanced for; None for a roll
    /// kept before it was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<String>,
}

const MAGENTA: &str = "enlarger.m_filter_shift";
const YELLOW: &str = "enlarger.y_filter_shift";

impl Roll {
    /// `state` prints with the roll's filtration, not one of its own.
    pub fn filtered(&self, state: &EditState) -> bool {
        let (m, y) = (json::text(state.value(MAGENTA)), json::text(state.value(YELLOW)));
        unsafe { sys::negs_state_roll_filtered_of(self.magenta, self.yellow, m.as_ptr(), m.len(), y.as_ptr(), y.len()) == 1 }
    }

    /// Prep's word for a frame of this roll with its own filtration: why,
    /// when it's on another paper.
    pub fn own_filtration(&self, state: &EditState) -> &'static str {
        let (roll_paper, has) = match &self.paper {
            Some(p) => (p.as_str(), 1),
            None => ("", 0),
        };
        static_str(unsafe {
            sys::negs_state_roll_own_filtration(roll_paper.as_ptr(), roll_paper.len(), has, state.paper.as_ptr(), state.paper.len())
        })
    }
}

/// One state parsed by the core, freed on drop.
struct Handle(*mut sys::negs_state_t);

impl Handle {
    /// None when the core can't read what serde wrote: only for a state
    /// holding a non-finite f64 (crop, border, roll, match ev), which serde
    /// writes as null and can't read back either.
    fn of(state: &EditState) -> Option<Handle> {
        let text = serde_json::to_vec(state).expect("an EditState always writes");
        let p = unsafe { sys::negs_state_parse(text.as_ptr(), text.len()) };
        (!p.is_null()).then_some(Handle(p))
    }

    fn expect(state: &EditState) -> Handle {
        Self::of(state).expect("an edit state with a non-finite number reached the core")
    }

    fn state(&self) -> EditState {
        let bytes = take_buf(unsafe { sys::negs_state_write(self.0, 0) });
        serde_json::from_slice(&bytes).expect("the core writes an EditState serde reads")
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { sys::negs_state_free(self.0) };
    }
}

impl Default for EditState {
    fn default() -> Self {
        Handle(unsafe { sys::negs_state_default() }).state()
    }
}

impl PartialEq for EditState {
    fn eq(&self, other: &Self) -> bool {
        // A state the core can't read holds a NaN, which equals nothing.
        match (Handle::of(self), Handle::of(other)) {
            (Some(a), Some(b)) => unsafe { sys::negs_state_eq(a.0, b.0) == 1 },
            _ => false,
        }
    }
}

/// The exposure that matches the camera's brightness, solved live for the
/// settings it depends on (`BrightnessMatch` in `Parameters.swift`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrightnessMatch {
    /// The solved Prep exposure; None until the first solve.
    #[serde(default)]
    pub ev: Option<f64>,
    /// `match_inputs` the value was solved for.
    #[serde(default)]
    pub inputs: Option<Value>,
    /// The solve held exposure back to keep highlights off paper white.
    #[serde(default)]
    pub highlight_limited: bool,
}

impl PartialEq for BrightnessMatch {
    fn eq(&self, other: &Self) -> bool {
        // As the core's match_equal: the options and flag as they are, the
        // inputs by json::same (in the core).
        self.ev == other.ev
            && self.highlight_limited == other.highlight_limited
            && match (&self.inputs, &other.inputs) {
                (Some(a), Some(b)) => json::same(a, b),
                (a, b) => a.is_none() && b.is_none(),
            }
    }
}

pub const PREP_EXPOSURE: &str = "prep.exposure_ev";

/// What a brightness match depends on: everything that changes how bright
/// the developed photograph is, except the user's own brightness edits.
pub fn match_inputs(state: &EditState) -> Value {
    let h = Handle::expect(state);
    json::read(&take_buf(unsafe { sys::negs_state_match_inputs(h.0) }))
}

impl EditState {
    /// The state without presentation-only settings, to decide whether an
    /// edit needs a new render (see windows/src/state.rs).
    pub fn rendered(&self) -> EditState {
        let h = Handle::expect(self);
        Handle(unsafe { sys::negs_state_rendered(h.0) }).state()
    }

    pub fn value(&self, path: &str) -> &Value {
        json::get(&self.params, path)
    }

    pub fn rotation(&self) -> i32 {
        let t = json::text(self.value("io.rotate_cw"));
        unsafe { sys::negs_state_rotation_of(t.as_ptr(), t.len()) }
    }

    /// Mirrored left to right after the crop, before the turns (`io.flip`).
    pub fn flip(&self) -> bool {
        let t = json::text(self.value(FLIP));
        unsafe { sys::negs_state_flip_of(t.as_ptr(), t.len()) == 1 }
    }

    pub fn orientation(&self) -> crate::crop::Orientation {
        crate::crop::Orientation::new(self.rotation(), self.flip())
    }
}

/// Flip (`IoParams::flip`).
pub const FLIP: &str = "io.flip";

/// Image Size (`ImageSize.swift`): the resolution the film develops at.
pub const SIZE_TARGET: &str = "io.photo_long_side_px";
pub const SIZE_FACTOR: &str = "io.upscale_factor";

/// The camera's own lens corrections for a RAW (`io.lens_correction`).
pub const LENS_CORRECTION: &str = "io.lens_correction.enabled";

/// A saved edit's params on today's defaults. A session from before lens
/// corrections was developed without them, so they stay off.
pub fn restored_params(defaults: &Value, saved: &Value, nullable: &[String]) -> Value {
    let (d, s, n) = (json::text(defaults), json::text(saved), json::strs(nullable));
    let mut out = json::empty();
    let ok = unsafe { sys::negs_state_restored_params(d.as_ptr(), d.len(), s.as_ptr(), s.len(), n.as_ptr(), n.len(), &mut out) };
    json::take(ok, out)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SizeMode {
    Original,
    LongestSide,
}

pub fn size_mode(params: &Value) -> SizeMode {
    if size_target(params).is_some() { SizeMode::LongestSide } else { SizeMode::Original }
}

pub fn size_target(params: &Value) -> Option<u32> {
    let t = json::text(json::get(params, SIZE_TARGET));
    let mut out = 0u32;
    (unsafe { sys::negs_state_size_target_of(t.as_ptr(), t.len(), &mut out) } == 1).then_some(out)
}

/// The frame `params` develops for a `source` of that size, and the
/// photograph `crop` cuts from it, with the engine's own resolver.
pub fn resolve_size(
    source: (u32, u32),
    params: &Value,
    crop: &CropSettings,
) -> Result<spektrafilm_io::image_size::ResolvedSize, spektrafilm_io::image_size::SizeError> {
    let engine = crop.cropped().then(|| crop.engine());
    spektrafilm_io::image_size::resolve(source, 1.0, size_target(params), engine.as_ref())
}

// MARK: Phase 4a

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn constants_are_the_cores() {
        let constant = |i| static_str(unsafe { sys::negs_state_constant(i) });
        assert_eq!([PREP_EXPOSURE, FLIP, SIZE_TARGET, SIZE_FACTOR, LENS_CORRECTION].map(String::from), [0, 1, 2, 3, 4].map(|i| constant(i).to_string()));
        let d = EditState::default();
        assert_eq!((d.film.as_str(), d.paper.as_str()), (constant(5), constant(6)));
        assert_eq!(d.params, json!({}));
        assert!(unsafe { sys::negs_state_constant(9) }.is_null());
    }

    #[test]
    fn sessions_before_lens_corrections_keep_them_off() {
        let defaults = serde_json::json!({"io": {"lens_correction": {"enabled": true}, "rotate_cw": 0}});
        let old = serde_json::json!({"io": {"rotate_cw": 1}});
        assert_eq!(restored_params(&defaults, &old, &[]), serde_json::json!({"io": {"lens_correction": {"enabled": false}, "rotate_cw": 1}}));
        for saved in [true, false] {
            let current = serde_json::json!({"io": {"lens_correction": {"enabled": saved}, "rotate_cw": 1}});
            assert_eq!(json::get(&restored_params(&defaults, &current, &[]), LENS_CORRECTION), &Value::Bool(saved));
        }
    }

    #[test]
    fn rendered_state_ignores_presentation() {
        let mut state = EditState { params: json!({"io": {"rotate_cw": 1}, "a": 2}), ..Default::default() };
        let rendered = state.rendered();
        state.border.enabled = true;
        state.crop.angle = 3.0;
        json::set(&mut state.params, "io.rotate_cw", json!(2));
        assert_eq!(state.rendered(), rendered);
        json::set(&mut state.params, "a", json!(2.5));
        assert_ne!(state.rendered(), rendered);
    }

    /// The crop's extent is the render's identity only with a target, as on the Mac.
    #[test]
    fn rendered_state_keeps_the_crop_extent_only_for_longest_side() {
        let mut state = EditState { params: json!({"io": {"rotate_cw": 0, "photo_long_side_px": null}}), ..Default::default() };
        let original = state.rendered();
        state.crop.rect = crate::crop::Rect::new(0.1, 0.2, 0.5, 0.5);
        assert_eq!(state.rendered(), original);
        json::set(&mut state.params, SIZE_TARGET, json!(4000));
        let sized = state.rendered();
        state.crop.rect.x = 0.3;
        state.crop.angle = -2.0;
        assert_eq!(state.rendered(), sized);
        state.crop.rect.width = 0.6;
        assert_ne!(state.rendered(), sized);
    }

    #[test]
    fn image_size_modes_and_resolution_follow_the_engine() {
        let params = |target: Value| json!({"io": {"photo_long_side_px": target}});
        assert_eq!(size_mode(&params(Value::Null)), SizeMode::Original);
        assert_eq!(size_mode(&params(json!(6000))), SizeMode::LongestSide);
        let centre = CropSettings { rect: crate::crop::Rect::new(0.25, 0.25, 0.5, 0.5), ..Default::default() };
        let size = resolve_size((6000, 4000), &params(json!(6000)), &centre).unwrap();
        assert_eq!((size.working, size.photo), ((12000, 8000), (6000, 4000)));
        assert_eq!(resolve_size((6000, 4000), &params(Value::Null), &centre).unwrap().working, (6000, 4000));
    }

    /// The match depends on what changes the developed brightness, not on
    /// the user's own exposure edits or the frame.
    #[test]
    fn match_inputs_leave_out_the_users_own_brightness() {
        let mut state = EditState { params: json!({"prep": {"exposure_ev": 0.5}, "enlarger": {"print_exposure": 1.0, "m_filter_shift": 0.0},
            "io": {"rotate_cw": 0, "output_color_space": "sRGB"}, "camera": {"exposure_compensation_ev": 0.0}}), ..Default::default() };
        let inputs = match_inputs(&state);
        for (path, value) in [("prep.exposure_ev", json!(2.0)), ("prep.shadows", json!(30)), ("enlarger.print_exposure", json!(1.4)),
            ("io.rotate_cw", json!(2)), ("io.output_color_space", json!("Display P3"))] {
            let mut edited = state.clone();
            json::set(&mut edited.params, path, value);
            assert!(json::same(&match_inputs(&edited), &inputs), "{path}");
        }
        state.crop.angle = 3.0;
        assert!(json::same(&match_inputs(&state), &inputs));
        for (path, value) in [("camera.exposure_compensation_ev", json!(1.0)), ("enlarger.m_filter_shift", json!(5.0))] {
            let mut edited = state.clone();
            json::set(&mut edited.params, path, value);
            assert!(!json::same(&match_inputs(&edited), &inputs), "{path}");
        }
        let mut paper = state.clone();
        paper.paper = "other".into();
        assert!(!json::same(&match_inputs(&paper), &inputs));
        // The match never re-renders by itself, and saves with the session.
        let mut matched = state.clone();
        matched.brightness_match = Some(BrightnessMatch { ev: Some(0.5), inputs: Some(inputs), highlight_limited: false });
        assert_eq!(matched.rendered(), state.rendered());
        let text = serde_json::to_string(&matched).unwrap();
        assert!(text.contains(r#""brightnessMatch":{"ev":0.5"#));
        assert_eq!(serde_json::from_str::<EditState>(&text).unwrap(), matched);
        assert!(!serde_json::to_string(&state).unwrap().contains("brightnessMatch"));
    }

    #[test]
    fn rolls_rotation_and_nan() {
        let state = EditState { params: json!({"enlarger": {"m_filter_shift": 3, "y_filter_shift": -1.5}, "io": {"rotate_cw": 7.9, "flip": true}}), ..Default::default() };
        let roll = Roll { id: "r".into(), magenta: 3.0, yellow: -1.5, paper: Some("other".into()) };
        assert!(roll.filtered(&state));
        assert!(!Roll { yellow: 1.5, ..roll.clone() }.filtered(&state));
        assert_eq!(roll.own_filtration(&state), "On another paper than its roll, so it has its own filtration.");
        assert_eq!(Roll { paper: None, ..roll }.own_filtration(&state), "This frame has its own filtration.");
        assert_eq!((state.rotation(), state.flip(), state.orientation().turns), (7, true, 3));
        let mut nan = state.clone();
        nan.crop.angle = f64::NAN;
        assert_ne!(nan, nan.clone());
        assert_eq!(state, state.clone());
    }
}
