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
use std::os::raw::c_char;
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

// Stocks and colour filters stay Rust structs (the model keeps them); they
// cross as negs_stock_t / negs_color_filter_t borrowing their strings. The
// tables stay Rust `const`s (`ADJUSTMENT_GROUPS` borrows `&'static`
// controls), checked against the core's in the tests. `choosing` and
// `pair_film` ask the core what to set and set it in place: a preview tile
// chooses each film once per state, too often to send the whole state.

fn str_t(text: &str) -> sys::negs_str_t {
    sys::negs_str_t { data: text.as_ptr(), len: text.len() }
}

/// A negs_str_t the core filled, borrowed for as long as its source.
pub(crate) unsafe fn borrowed<'a>(s: sys::negs_str_t) -> &'a str {
    if s.data.is_null() || s.len == 0 {
        return "";
    }
    std::str::from_utf8(unsafe { std::slice::from_raw_parts(s.data, s.len) }).expect("the core writes UTF-8")
}

/// Words the core writes into a buffer (returning their whole length).
pub(crate) fn words_from(write: impl Fn(*mut u8, usize) -> usize) -> String {
    let mut buf = [0u8; 256];
    let n = write(buf.as_mut_ptr(), buf.len());
    let bytes = if n <= buf.len() {
        buf[..n].to_vec()
    } else {
        let mut big = vec![0u8; n];
        let m = write(big.as_mut_ptr(), big.len());
        big.truncate(m);
        big
    };
    String::from_utf8(bytes).expect("the core writes UTF-8")
}

fn bit(b: bool) -> u8 {
    b as u8
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stock {
    pub id: String,
    pub name: String,
    pub paper: bool,
    pub positive: bool,
    pub bw: bool,
    pub target: String,
    pub times: Vec<f64>,
    /// The resolved grain preset, per field of `GRAIN_STOCK_FIELDS`.
    pub grain: Value,
    /// Ported from Fotufilm; and, for those, whether from the manufacturer's
    /// data rather than modelled from sample photographs (Settings › Fotufilm films).
    pub fotufilm: bool,
    pub manufacturer_data: bool,
    /// Prepared by Negs itself from datasheets (Settings › Show Negs-built films).
    pub negs_built: bool,
    /// A motion-picture print film (catalog `cine_print`).
    pub cine_print: bool,
}

/// The stock as the core reads it, borrowing its strings (no grain: only a
/// catalog reads it, through `sys_stock_with_grain`).
pub(crate) fn sys_stock(s: &Stock) -> sys::negs_stock_t {
    sys::negs_stock_t {
        id: str_t(&s.id),
        name: str_t(&s.name),
        target: str_t(&s.target),
        grain: str_t(""),
        times: s.times.as_ptr(),
        times_len: s.times.len(),
        paper: bit(s.paper),
        positive: bit(s.positive),
        bw: bit(s.bw),
        fotufilm: bit(s.fotufilm),
        manufacturer_data: bit(s.manufacturer_data),
        negs_built: bit(s.negs_built),
        cine_print: bit(s.cine_print),
    }
}

/// With the grain preset's JSON (`grain`, which the result borrows).
pub fn sys_stock_with_grain<'a>(s: &'a Stock, grain: &'a [u8]) -> sys::negs_stock_t {
    let mut out = sys_stock(s);
    out.grain = sys::negs_str_t { data: grain.as_ptr(), len: grain.len() };
    out
}

impl Stock {
    pub fn new(value: &Value) -> Self {
        let text = json::text(value);
        let mut out: sys::negs_stock_t = unsafe { std::mem::zeroed() };
        let mut storage = json::empty();
        let ok = unsafe { sys::negs_stock_read(text.as_ptr(), text.len(), &mut out, &mut storage) };
        assert!(ok == 1, "the core refused serde_json's own text");
        let times = if out.times.is_null() { vec![] } else { unsafe { std::slice::from_raw_parts(out.times, out.times_len) }.to_vec() };
        let grain = unsafe { borrowed(out.grain) };
        let stock = Self {
            id: unsafe { borrowed(out.id) }.to_string(),
            name: unsafe { borrowed(out.name) }.to_string(),
            paper: out.paper != 0,
            positive: out.positive != 0,
            bw: out.bw != 0,
            target: unsafe { borrowed(out.target) }.to_string(),
            times,
            grain: if grain.is_empty() { Value::Null } else { json::read(grain.as_bytes()) },
            fotufilm: out.fotufilm != 0,
            manufacturer_data: out.manufacturer_data != 0,
            negs_built: out.negs_built != 0,
            cine_print: out.cine_print != 0,
        };
        take_buf(storage);
        stock
    }

    pub fn is_motion_picture_print(&self) -> bool {
        unsafe { sys::negs_stock_is_motion_picture_print(&sys_stock(self)) == 1 }
    }
}

/// A camera colour filter from the catalog (`ColorFilter` in
/// `Parameters.swift`, issue #21): its params key, the colour it's named
/// by, the measured glass, and the glass's colour for its disc.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorFilter {
    pub key: String,
    pub name: String,
    pub glass: String,
    pub colour: u32,
}

pub(crate) fn sys_filter(f: &ColorFilter) -> sys::negs_color_filter_t {
    sys::negs_color_filter_t { key: str_t(&f.key), name: str_t(&f.name), glass: str_t(&f.glass), colour: f.colour }
}

impl ColorFilter {
    pub const PATH: &'static str = "camera.color_filter";
    pub const NONE: &'static str = "none";

    pub fn new(value: &Value) -> Self {
        let text = json::text(value);
        let mut out: sys::negs_color_filter_t = unsafe { std::mem::zeroed() };
        let mut storage = json::empty();
        let ok = unsafe { sys::negs_color_filter_read(text.as_ptr(), text.len(), &mut out, &mut storage) };
        assert!(ok == 1, "the core refused serde_json's own text");
        let filter = unsafe {
            Self { key: borrowed(out.key).into(), name: borrowed(out.name).into(), glass: borrowed(out.glass).into(), colour: out.colour }
        };
        take_buf(storage);
        filter
    }

    /// What each filter does to a black-and-white photograph.
    pub fn effect(key: &str) -> &'static str {
        static_str(unsafe { sys::negs_color_filter_effect(key.as_ptr(), key.len()) })
    }

    /// What the filter does, then what it costs the film
    /// (`ColorFilterPicker.hint`): "… Takes 1,3 stops on Ilford HP5 Plus;
    /// auto exposure gives it." `decimal` is the locale's separator.
    pub fn hint(filter: Option<&ColorFilter>, stops: Option<f64>, film: &str, metered: bool, decimal: char) -> String {
        let f = filter.map(sys_filter);
        let mut out = json::empty();
        unsafe {
            sys::negs_color_filter_hint(
                f.as_ref().map_or(std::ptr::null(), |f| f),
                stops.is_some() as u8,
                stops.unwrap_or(0.0),
                film.as_ptr(),
                film.len(),
                metered as u8,
                decimal as u32,
                &mut out,
            )
        };
        String::from_utf8(take_buf(out)).expect("the core writes UTF-8")
    }

    /// The disc's tooltip.
    pub fn help(filter: Option<&ColorFilter>) -> String {
        let f = filter.map(sys_filter);
        let mut out = json::empty();
        unsafe { sys::negs_color_filter_help(f.as_ref().map_or(std::ptr::null(), |f| f), &mut out) };
        String::from_utf8(take_buf(out)).expect("the core writes UTF-8")
    }
}

/// Enlarger › Shifts move (`FilterLink` in `Parameters.swift`): how the
/// magenta and yellow shift sliders move each other. Together is the
/// red↔cyan axis. Linked, they also reset together. A preference of the app,
/// not of the edit: the edit still holds just the two shifts, so presets
/// are unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterLink {
    Separate,
    Together,
    Opposite,
}

impl FilterLink {
    pub const ALL: [FilterLink; 3] = [FilterLink::Separate, FilterLink::Together, FilterLink::Opposite];
    pub const MAGENTA: &'static str = "enlarger.m_filter_shift";
    pub const YELLOW: &'static str = "enlarger.y_filter_shift";
    pub const RANGE: (f64, f64) = (-50.0, 50.0);

    fn of(index: u32) -> Self {
        Self::ALL[index as usize]
    }

    /// As the Mac stores it (`filterShiftLink`); anything else is Separately.
    pub fn from_key(key: &str) -> Self {
        Self::of(unsafe { sys::negs_filter_link_from_key(key.as_ptr(), key.len()) })
    }

    pub fn key(self) -> &'static str {
        static_str(unsafe { sys::negs_filter_link_word(self as u32, 0) })
    }

    pub fn title(self) -> &'static str {
        static_str(unsafe { sys::negs_filter_link_word(self as u32, 1) })
    }

    pub fn links(path: &str) -> bool {
        unsafe { sys::negs_filter_link_links(path.as_ptr(), path.len()) == 1 }
    }

    /// The two shifts after moving `path` to `value` (already snapped). The
    /// other moves by the same change, so their gap (Together) or sum
    /// (Opposite) holds, with no jump on switching; at either end of the
    /// range both stop.
    pub fn shifts(self, path: &str, value: f64, magenta: f64, yellow: f64) -> (f64, f64) {
        let mut out = [0.0; 2];
        unsafe { sys::negs_filter_link_shifts(self as u32, path.as_ptr(), path.len(), value, magenta, yellow, &mut out) };
        (out[0], out[1])
    }
}

/// Settings › Show Negs-built films (`NegsBuiltFilms` in `Parameters.swift`):
/// the films Negs prepared itself from datasheets stay out of the film menu and
/// grid until shown. Only the lists, as for Fotufilm films.
pub fn lists_negs_built(stock: &Stock, shown: bool) -> bool {
    unsafe { sys::negs_stock_lists_negs_built(&sys_stock(stock), shown as u8) == 1 }
}

pub const NEGS_BUILT_HINT: &str = "Films Negs prepared itself from Fujifilm’s datasheets, still being checked: the Eterna cine films; Pro 160S, 160C and 800Z, Superia 1600 and True Definition 400; the Velvia 100F, Provia 400F and 400X, Sensia and 64T slides; and the Eterna-CP print films. Photos already using one keep it.";

/// Settings › Fotufilm films (`FotufilmFilms` in `Parameters.swift`): which
/// of the films ported from Fotufilm the film menu and grid list. Only the
/// lists: an edit or a preset with a hidden film keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FotufilmFilms {
    All,
    Datasheets,
    Off,
}

impl FotufilmFilms {
    pub const ALL: [FotufilmFilms; 3] = [FotufilmFilms::All, FotufilmFilms::Datasheets, FotufilmFilms::Off];

    /// As the Mac stores it (`fotufilmFilms`); anything else is All.
    pub fn from_key(key: &str) -> Self {
        Self::ALL[unsafe { sys::negs_fotufilm_from_key(key.as_ptr(), key.len()) } as usize]
    }

    pub fn key(self) -> &'static str {
        static_str(unsafe { sys::negs_fotufilm_word(self as u32, 0) })
    }

    pub fn title(self) -> &'static str {
        static_str(unsafe { sys::negs_fotufilm_word(self as u32, 1) })
    }

    pub fn lists(self, stock: &Stock) -> bool {
        unsafe { sys::negs_fotufilm_lists(self as u32, &sys_stock(stock)) == 1 }
    }
}

/// Grain arrays that stay null until edited, so the film's preset from the
/// catalog applies (`Stock.grainFields` in `Parameters.swift`).
pub const GRAIN_STOCK_FIELDS: [&str; 4] = ["rms_granularity", "density_min", "uniformity", "particle_scale_sublayers"];

/// The DIR gammas and halation baseline, null until set so the film's presets
/// apply (`Stock.dirHalationPaths` in `Parameters.swift`).
pub const DIR_HALATION_STOCK_PATHS: [&str; 6] = [
    "film_render.dir_couplers.gamma_samelayer_rgb",
    "film_render.dir_couplers.gamma_interlayer_r_to_gb",
    "film_render.dir_couplers.gamma_interlayer_g_to_rb",
    "film_render.dir_couplers.gamma_interlayer_b_to_rg",
    "film_render.halation.halation_strength",
    "film_render.halation.halation_first_sigma_um",
];

/// A slider on one lane of a grain preset array:
/// `film_render.grain.<field>.<index>`.
pub fn grain_lane(path: &str) -> Option<(&str, usize)> {
    let (mut field, mut lane) = (0u32, 0u64);
    if unsafe { sys::negs_grain_lane(path.as_ptr(), path.len(), &mut field, &mut lane) } == 0 {
        return None;
    }
    Some((GRAIN_STOCK_FIELDS[field as usize], usize::try_from(lane).ok()?))
}

/// B&W films have one emulsion: per-channel lanes past the first are hidden.
pub fn color_only(path: &str) -> bool {
    unsafe { sys::negs_color_only(path.as_ptr(), path.len()) == 1 }
}

/// The first lane's title on a B&W film, without its colour.
pub fn bw_title(path: &str) -> Option<&'static str> {
    let p = unsafe { sys::negs_bw_title(path.as_ptr(), path.len()) };
    (!p.is_null()).then(|| static_str(p))
}

/// Sets the value at a dotted path in place, as `json::set` does (missing
/// keys become objects; an index past an array's end or a scalar on the
/// way changes nothing): applies the core's plan without sending the state.
fn set_in_place(value: &mut Value, path: &str, new: Value) {
    let mut current = value;
    for key in path.split('.') {
        current = match current {
            Value::Object(map) => map.entry(key.to_string()).or_insert_with(|| Value::Object(serde_json::Map::new())),
            Value::Array(items) => match key.parse::<usize>().ok().and_then(|i| items.get_mut(i)) {
                Some(child) => child,
                None => return,
            },
            _ => return,
        };
    }
    *current = new;
}

/// The params `negs_choice_t::nulls` names, in its bits' order.
fn choice_paths() -> &'static [*const c_char] {
    let mut n = 0;
    let p = unsafe { sys::negs_state_words(7, &mut n) };
    unsafe { std::slice::from_raw_parts(p, n) }
}

fn apply(state: &mut EditState, plan: &sys::negs_choice_t) {
    let (film, paper) = unsafe { (borrowed(plan.film), borrowed(plan.paper)) };
    if state.film != film {
        state.film = film.to_string();
    }
    if state.paper != paper {
        state.paper = paper.to_string();
    }
    for (i, &path) in choice_paths().iter().enumerate() {
        if plan.nulls & (1 << i) != 0 {
            set_in_place(&mut state.params, static_str(path), Value::Null);
        }
    }
    if plan.sets_scan != 0 {
        set_in_place(&mut state.params, "io.scan_film", Value::Bool(plan.scan != 0));
    }
}

/// The edit with stock `id` chosen, exactly as choosing it from the menu or
/// a preview tile makes it (`PreviewRules.choosing`). A film resets its
/// development time and its grain, DIR and halation presets, sets scanning
/// for slides and brings its own paper unless `keep_paper` (Settings › Keep
/// paper when changing film); a paper keeps the film.
pub fn choosing(state: &EditState, id: &str, paper: bool, keep_paper: bool, stocks: &[Stock]) -> EditState {
    let list: Vec<sys::negs_stock_t> = stocks.iter().map(sys_stock).collect();
    let mut plan: sys::negs_choice_t = unsafe { std::mem::zeroed() };
    unsafe {
        sys::negs_choosing_plan(
            state.film.as_ptr(),
            state.film.len(),
            state.paper.as_ptr(),
            state.paper.len(),
            id.as_ptr(),
            id.len(),
            paper as u8,
            keep_paper as u8,
            list.as_ptr(),
            list.len(),
            &mut plan,
        )
    };
    let mut next = state.clone();
    apply(&mut next, &plan);
    next
}

/// Scanning for slides, and the film's own paper unless `paper` is false.
pub fn pair_film(state: &mut EditState, stocks: &[Stock], paper: bool) {
    let list: Vec<sys::negs_stock_t> = stocks.iter().map(sys_stock).collect();
    let mut plan: sys::negs_choice_t = unsafe { std::mem::zeroed() };
    unsafe {
        sys::negs_pair_film_plan(
            state.film.as_ptr(),
            state.film.len(),
            state.paper.as_ptr(),
            state.paper.len(),
            list.as_ptr(),
            list.len(),
            paper as u8,
            &mut plan,
        )
    };
    // The plan borrows the state's own strings: copy them out first.
    let (film, paper) = unsafe { (borrowed(plan.film).to_string(), borrowed(plan.paper).to_string()) };
    plan.film = str_t(&film);
    plan.paper = str_t(&paper);
    apply(state, &plan);
}

/// Stocks of one kind, in catalog order, for sectioned stock menus.
pub struct StockGroup<'a> {
    pub title: &'static str,
    pub stocks: Vec<&'a Stock>,
}

fn groups_from<'a>(stocks: &[&'a Stock], groups: &[sys::negs_stock_group_t], places: &[u32]) -> Vec<StockGroup<'a>> {
    groups
        .iter()
        .map(|g| StockGroup { title: static_str(g.title), stocks: places[g.start..g.start + g.count].iter().map(|&i| stocks[i as usize]).collect() })
        .collect()
}

pub fn film_groups<'a>(films: &[&'a Stock], papers: &[&'a Stock]) -> Vec<StockGroup<'a>> {
    let (f, p): (Vec<_>, Vec<_>) = (films.iter().map(|s| sys_stock(s)).collect(), papers.iter().map(|s| sys_stock(s)).collect());
    let mut groups: [sys::negs_stock_group_t; 4] = unsafe { std::mem::zeroed() };
    let mut places = vec![0u32; films.len()];
    let n = unsafe { sys::negs_film_groups(f.as_ptr(), f.len(), p.as_ptr(), p.len(), groups.as_mut_ptr(), groups.len(), places.as_mut_ptr()) };
    groups_from(films, &groups[..n], &places)
}

pub fn paper_groups<'a>(papers: &[&'a Stock]) -> Vec<StockGroup<'a>> {
    let p: Vec<_> = papers.iter().map(|s| sys_stock(s)).collect();
    let mut groups: [sys::negs_stock_group_t; 3] = unsafe { std::mem::zeroed() };
    let mut places = vec![0u32; papers.len()];
    let n = unsafe { sys::negs_paper_groups(p.as_ptr(), p.len(), groups.as_mut_ptr(), groups.len(), places.as_mut_ptr()) };
    groups_from(papers, &groups[..n], &places)
}

/// The film search's `contains`, ignoring case and diacritics as
/// `localizedStandardContains` compares; the host registers its own once
/// (Windows: `FindNLSStringEx` in the invariant locale). Without one the
/// core searches bytes.
pub fn set_contains(contains: sys::negs_contains_fn) {
    unsafe { sys::negs_text_set_contains(contains) }
}

/// The stocks matching every word of `query` in their name, id or group
/// title, ignoring case and accents ("portra 160", "kodak slide"); groups
/// left empty drop out (`StockGroup.matching`). An empty query keeps them all.
pub fn matching<'a>(query: &str, groups: Vec<StockGroup<'a>>) -> Vec<StockGroup<'a>> {
    let titles: Vec<sys::negs_str_t> = groups.iter().map(|g| str_t(g.title)).collect();
    let counts: Vec<usize> = groups.iter().map(|g| g.stocks.len()).collect();
    let stocks: Vec<sys::negs_stock_t> = groups.iter().flat_map(|g| g.stocks.iter().map(|s| sys_stock(s))).collect();
    let mut keep = vec![1u8; stocks.len()];
    let searched =
        unsafe { sys::negs_stock_matching(query.as_ptr(), query.len(), titles.as_ptr(), counts.as_ptr(), groups.len(), stocks.as_ptr(), keep.as_mut_ptr()) };
    if searched == 0 {
        return groups;
    }
    let mut kept = keep.into_iter();
    groups
        .into_iter()
        .filter_map(|group| {
            let stocks: Vec<&Stock> = group.stocks.into_iter().filter(|_| kept.next() == Some(1)).collect();
            (!stocks.is_empty()).then_some(StockGroup { title: group.title, stocks })
        })
        .collect()
}

/// Human-readable names for engine option identifiers.
pub fn option_title(option: &str) -> String {
    words_from(|out, cap| unsafe { sys::negs_option_title(option.as_ptr(), option.len(), out, cap) })
}

/// Sliders move in steps; the engine expects the stepped value (integer
/// fields reject fractions). Like `EditorModel.snap`.
pub fn snap(value: f64, low: f64, high: f64, step: f64) -> f64 {
    unsafe { sys::negs_snap(value, low, high, step) }
}

/// Decimal places a value with this step shows: as many as the step has
/// (0.05 shows two).
pub fn decimals(step: f64) -> usize {
    unsafe { sys::negs_decimals(step) }
}

/// A value as the inspector shows it: up to the step's decimals, no
/// trailing zeros.
pub fn format_value(value: f64, step: f64) -> String {
    words_from(|out, cap| unsafe { sys::negs_format_value(value, step, out, cap) })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Adjustment {
    Slider { title: &'static str, path: &'static str, low: f64, high: f64, step: f64 },
    Toggle { title: &'static str, path: &'static str },
    Choice { title: &'static str, path: &'static str, options: &'static [&'static str] },
    /// Solves the enlarger's M/Y filter shifts so a mid-grey prints neutral.
    BalancePrint { title: &'static str, help: &'static str },
    /// Enlarger › Shifts move: a preference, not an edit (`FilterLink`).
    FilterLink { help: &'static str },
}

impl Adjustment {
    /// The parameter a control edits; empty for the Balance Print button.
    pub fn path(&self) -> &'static str {
        match *self {
            Adjustment::Slider { path, .. } | Adjustment::Toggle { path, .. } | Adjustment::Choice { path, .. } => path,
            Adjustment::BalancePrint { .. } | Adjustment::FilterLink { .. } => "",
        }
    }
}

const fn slider(title: &'static str, path: &'static str, low: f64, high: f64, step: f64) -> Adjustment {
    Adjustment::Slider { title, path, low, high, step }
}
const fn toggle(title: &'static str, path: &'static str) -> Adjustment {
    Adjustment::Toggle { title, path }
}
const fn choice(title: &'static str, path: &'static str, options: &'static [&'static str]) -> Adjustment {
    Adjustment::Choice { title, path, options }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Camera,
    Film,
    Print,
    Scan,
}

pub struct AdjustmentGroup {
    pub title: &'static str,
    /// A Segoe MDL2 Assets glyph for the header.
    pub section: Section,
    /// Switches the whole effect on and off from the group's header.
    pub enable: Option<&'static str>,
    /// How many of the leading controls the Lab shows without Show all
    /// controls; a group with none shows only then (or once it's changed).
    pub basic: usize,
    pub controls: &'static [Adjustment],
}

/// The same groups, paths, ranges and steps as `adjustmentGroups` in
/// `Inspector.swift`.
pub const ADJUSTMENT_GROUPS: &[AdjustmentGroup] = &[
    AdjustmentGroup {
        title: "Lens Diffusion",
        section: Section::Camera,
        enable: Some("camera.diffusion_filter.active"),
        basic: 2,
        controls: &[
            choice("Filter", "camera.diffusion_filter.filter_family", &["black_pro_mist", "glimmerglass", "pro_mist", "cinebloom"]),
            slider("Strength", "camera.diffusion_filter.strength", 0.0, 2.0, 0.01),
            slider("Spatial scale", "camera.diffusion_filter.spatial_scale", 0.1, 3.0, 0.01),
            slider("Halo warmth", "camera.diffusion_filter.halo_warmth", -1.5, 1.5, 0.01),
            slider("Core intensity", "camera.diffusion_filter.core_intensity", 0.0, 2.0, 0.01),
            slider("Halo intensity", "camera.diffusion_filter.halo_intensity", 0.0, 2.0, 0.01),
            slider("Bloom intensity", "camera.diffusion_filter.bloom_intensity", 0.0, 2.0, 0.01),
            slider("Halo size", "camera.diffusion_filter.halo_size", 0.1, 3.0, 0.01),
            slider("Bloom size", "camera.diffusion_filter.bloom_size", 0.1, 3.0, 0.01),
        ],
    },
    AdjustmentGroup {
        title: "Film Curves",
        section: Section::Film,
        enable: Some("film_render.density_curves_morph.active"),
        basic: 1,
        controls: &[
            slider("Gamma", "film_render.density_curves_morph.gamma_factor", 0.5, 2.0, 0.01),
            slider("Fast gamma", "film_render.density_curves_morph.gamma_factor_fast", 0.5, 2.0, 0.01),
            slider("Slow gamma", "film_render.density_curves_morph.gamma_factor_slow", 0.5, 2.0, 0.01),
            slider("Red gamma", "film_render.density_curves_morph.gamma_factor_red", 0.5, 2.0, 0.01),
            slider("Green gamma", "film_render.density_curves_morph.gamma_factor_green", 0.5, 2.0, 0.01),
            slider("Blue gamma", "film_render.density_curves_morph.gamma_factor_blue", 0.5, 2.0, 0.01),
            slider("Developer exhaustion", "film_render.density_curves_morph.developer_exhaustion", 0.0, 1.0, 0.01),
        ],
    },
    AdjustmentGroup {
        title: "Grain",
        section: Section::Film,
        enable: Some("film_render.grain.active"),
        basic: 3,
        controls: &[
            slider("Red granularity", "film_render.grain.rms_granularity.0", 0.0, 80.0, 0.1),
            slider("Green granularity", "film_render.grain.rms_granularity.1", 0.0, 80.0, 0.1),
            slider("Blue granularity", "film_render.grain.rms_granularity.2", 0.0, 80.0, 0.1),
            slider("Red uniformity", "film_render.grain.uniformity.0", 0.0, 1.0, 0.01),
            slider("Green uniformity", "film_render.grain.uniformity.1", 0.0, 1.0, 0.01),
            slider("Blue uniformity", "film_render.grain.uniformity.2", 0.0, 1.0, 0.01),
            slider("Coarse grain size", "film_render.grain.particle_scale_sublayers.0", 0.05, 2.0, 0.01),
            slider("Medium grain size", "film_render.grain.particle_scale_sublayers.1", 0.05, 2.0, 0.01),
            slider("Fine grain size", "film_render.grain.particle_scale_sublayers.2", 0.05, 2.0, 0.01),
            slider("Dye clouds (µm)", "film_render.grain.blur_dye_clouds_um", 0.0, 5.0, 0.01),
            slider("Blur (px)", "film_render.grain.blur", 0.0, 3.0, 0.01),
            slider("Sharpen amount", "film_render.grain.mult_usm_amount", 0.0, 3.0, 0.01),
            slider("Sharpen radius (px)", "film_render.grain.mult_usm_sigma", 0.0, 3.0, 0.01),
            slider("Clump blur (µm)", "film_render.grain.micro_structure.0", 0.0, 1.0, 0.01),
            slider("Clump size (nm)", "film_render.grain.micro_structure.1", 0.0, 100.0, 1.0),
        ],
    },
    AdjustmentGroup {
        title: "Halation",
        section: Section::Film,
        enable: Some("film_render.halation.active"),
        basic: 1,
        controls: &[
            slider("Amount", "film_render.halation.halation_amount", 0.0, 3.0, 0.01),
            slider("Spatial scale", "film_render.halation.halation_spatial_scale", 0.1, 5.0, 0.01),
            slider("Scatter amount", "film_render.halation.scatter_amount", 0.0, 3.0, 0.01),
            slider("Scatter scale", "film_render.halation.scatter_spatial_scale", 0.1, 5.0, 0.01),
            slider("Bounces", "film_render.halation.halation_n_bounces", 1.0, 5.0, 1.0),
            slider("Bounce decay", "film_render.halation.halation_bounce_decay", 0.0, 1.0, 0.01),
        ],
    },
    AdjustmentGroup {
        title: "DIR Couplers",
        section: Section::Film,
        enable: Some("film_render.dir_couplers.active"),
        basic: 1,
        controls: &[
            slider("Amount", "film_render.dir_couplers.amount", 0.0, 2.0, 0.01),
            slider("Diffusion (µm)", "film_render.dir_couplers.diffusion_size_um", 0.0, 100.0, 0.1),
            slider("Tail (µm)", "film_render.dir_couplers.diffusion_tail_um", 0.0, 400.0, 1.0),
            slider("Tail weight", "film_render.dir_couplers.diffusion_tail_weight", 0.0, 1.0, 0.01),
        ],
    },
    AdjustmentGroup {
        title: "Film Base",
        section: Section::Film,
        enable: Some("film_render.base.active"),
        basic: 1,
        controls: &[
            slider("Density", "film_render.base.scale", 0.0, 2.0, 0.01),
            slider("Tilt", "film_render.base.tilt", -1.0, 1.0, 0.01),
            slider("Cyan", "film_render.base.cyan", 0.0, 2.0, 0.01),
            slider("Magenta", "film_render.base.magenta", 0.0, 2.0, 0.01),
            slider("Yellow", "film_render.base.yellow", 0.0, 2.0, 0.01),
        ],
    },
    AdjustmentGroup {
        title: "Enlarger",
        section: Section::Print,
        enable: None,
        basic: 5,
        controls: &[
            slider("Print exposure", "enlarger.print_exposure", 0.1, 5.0, 0.01),
            slider("Magenta shift", "enlarger.m_filter_shift", -50.0, 50.0, 0.1),
            slider("Yellow shift", "enlarger.y_filter_shift", -50.0, 50.0, 0.1),
            Adjustment::FilterLink {
                help: "How the magenta and yellow shifts move. Together moves both the same way: more of both makes the print more cyan, less of both redder. Opposite ways moves one up as the other goes down. Linked, resetting either resets both. Each keeps its own value, so presets are unchanged.",
            },
            Adjustment::BalancePrint {
                title: "Balance Print",
                help: "Sets the magenta and yellow shifts so a mid-grey prints neutral with the current film, bases, paper and enlarger. Use it after changing the film base.",
            },
            toggle("Exposure compensation", "enlarger.print_exposure_compensation"),
            slider("Preflash exposure", "enlarger.preflash_exposure", 0.0, 0.5, 0.01),
            slider("Preflash magenta", "enlarger.preflash_m_filter_shift", -50.0, 50.0, 0.1),
            slider("Preflash yellow", "enlarger.preflash_y_filter_shift", -50.0, 50.0, 0.1),
        ],
    },
    AdjustmentGroup {
        title: "Print Base",
        section: Section::Print,
        enable: Some("print_render.base.active"),
        basic: 1,
        controls: &[
            slider("Density", "print_render.base.scale", 0.0, 2.0, 0.01),
            slider("Cyan", "print_render.base.cyan", 0.0, 2.0, 0.01),
            slider("Magenta", "print_render.base.magenta", 0.0, 2.0, 0.01),
            slider("Yellow", "print_render.base.yellow", 0.0, 2.0, 0.01),
        ],
    },
    AdjustmentGroup {
        title: "Print Curves",
        section: Section::Print,
        enable: Some("print_render.density_curves_morph.active"),
        basic: 1,
        controls: &[
            slider("Gamma", "print_render.density_curves_morph.gamma_factor", 0.5, 2.0, 0.01),
            slider("Fast gamma", "print_render.density_curves_morph.gamma_factor_fast", 0.5, 2.0, 0.01),
            slider("Slow gamma", "print_render.density_curves_morph.gamma_factor_slow", 0.5, 2.0, 0.01),
            slider("Red gamma", "print_render.density_curves_morph.gamma_factor_red", 0.5, 2.0, 0.01),
            slider("Green gamma", "print_render.density_curves_morph.gamma_factor_green", 0.5, 2.0, 0.01),
            slider("Blue gamma", "print_render.density_curves_morph.gamma_factor_blue", 0.5, 2.0, 0.01),
            slider("Developer exhaustion", "print_render.density_curves_morph.developer_exhaustion", 0.0, 1.0, 0.01),
        ],
    },
    AdjustmentGroup {
        title: "Glare",
        section: Section::Print,
        enable: Some("print_render.glare.active"),
        basic: 1,
        controls: &[
            slider("Amount", "print_render.glare.percent", 0.0, 0.2, 0.001),
            slider("Roughness", "print_render.glare.roughness", 0.0, 2.0, 0.01),
            slider("Blur (px)", "print_render.glare.blur", 0.0, 5.0, 0.01),
        ],
    },
    AdjustmentGroup {
        title: "Scanner",
        section: Section::Scan,
        enable: None,
        basic: 0,
        controls: &[
            slider("Lens blur (px)", "scanner.lens_blur", 0.0, 5.0, 0.01),
            slider("Color noise reduction", "scanner.chroma_noise_reduction", 0.0, 100.0, 1.0),
            slider("Unsharp radius (px)", "scanner.unsharp_mask.0", 0.0, 3.0, 0.01),
            slider("Unsharp amount", "scanner.unsharp_mask.1", 0.0, 2.0, 0.01),
            toggle("White correction", "scanner.white_correction"),
            slider("White level", "scanner.white_level", 0.5, 1.0, 0.01),
            toggle("Black correction", "scanner.black_correction"),
            slider("Black level", "scanner.black_level", 0.0, 0.5, 0.01),
        ],
    },
    AdjustmentGroup {
        title: "Color Management",
        section: Section::Scan,
        enable: None,
        basic: 0,
        controls: &[
            choice("Output gamut", "io.output_gamut_compress.algorithm", &["off", "oklch", "oklrab", "cam16ucs", "aces_rgc"]),
            choice("Input gamut", "io.input_gamut_compress.algorithm", &["off", "xy"]),
            toggle("CAT16 adaptation", "settings.use_cat16"),
            choice("Spectral upsampling", "settings.rgb_to_raw_method", &["hanatos2025", "arctic2026beta04", "mallett2019"]),
        ],
    },
];

pub const METERING: &[&str] = &["average", "median", "center_weighted", "partial", "matrix", "multi_zone", "highlight_weighted"];
/// How a non-RAW photograph's values are read (`io.input_interpretation`).
pub const INPUT_MODES: &[&str] = &["auto", "assume", "manual"];
/// The colour spaces Assume offers, as `spektrafilm-io::colour::ASSUMABLE`.
pub const ASSUMED_SPACES: &[&str] = &["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB", "ITU-R BT.2020"];
/// Manual: the pipeline's own input colour space, as Negs 0.8 used it.
pub const MANUAL_SPACES: &[&str] = &["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB", "ITU-R BT.2020", "ACES2065-1", "ACEScg"];
pub const WHITE_BALANCE: &[&str] = &["as_shot", "daylight", "tungsten", "custom"];

pub const NEGATIVE_EXPOSURE: Adjustment = slider("Negative exposure (EV)", "camera.exposure_compensation_ev", -5.0, 5.0, 0.1);
pub const LENS_BLUR: Adjustment = slider("Lens blur (µm)", "camera.lens_blur_um", 0.0, 100.0, 0.1);
pub const WB_TEMPERATURE: Adjustment = slider("Temperature (K)", "io.raw_white_balance.temperature", 1667.0, 25000.0, 1.0);
pub const WB_TINT: Adjustment = slider("Tint", "io.raw_white_balance.tint", 0.01, 4.0, 0.01);

/// Prep's controls (`prepToneControls` and the rest in `Inspector.swift`),
/// applied to the scene before the film. Highlights keeps the recovered
/// extended travel (docs/prep.md).
pub const PREP_TONE: &[Adjustment] = &[
    slider("Highlights", "prep.highlights", -180.0, 100.0, 1.0),
    slider("Shadows", "prep.shadows", -100.0, 100.0, 1.0),
    slider("Contrast", "prep.contrast", -100.0, 100.0, 1.0),
    slider("Lift", "prep.lift", -100.0, 100.0, 1.0),
    slider("Gain", "prep.gain", -100.0, 100.0, 1.0),
];
pub const PREP_COLOR: &[Adjustment] = &[
    slider("Color Boost", "prep.color_boost", -100.0, 100.0, 1.0),
    slider("Saturation", "prep.saturation", -100.0, 100.0, 1.0),
];
pub const PREP_DETAIL: &[Adjustment] = &[slider("Midtone Detail", "prep.midtone_detail", -100.0, 100.0, 1.0)];
/// The engine keeps Prep's exposure within ±3 EV; the slider moves in 0.05 EV.
pub const PREP_EXPOSURE_RANGE: (f64, f64) = (-3.0, 3.0);
pub const PREP_EXPOSURE_STEP: f64 = 0.05;

/// Lab › Camera › Film format (`FilmFormatPicker`): stills formats by the long
/// edge of the image area, in mm. 135 stays at the engine's 35 (not the gate's
/// 36) so existing edits render as before; 6×4.5 and 6×6 share 56. Custom…
/// shows the slider.
pub const FILM_FORMAT: Adjustment = slider("Long edge (mm)", "camera.film_format_mm", 4.0, 120.0, 0.1);
pub const FILM_FORMATS: &[(&str, f64)] =
    &[("110", 17.0), ("Half frame", 24.0), ("35 mm (135)", 35.0), ("6×4.5 / 6×6", 56.0), ("6×7", 70.0), ("6×9", 84.0), ("4×5", 120.0)];


/// The format with this long edge; None reads as Custom.
pub fn film_format(mm: f64) -> Option<usize> {
    usize::try_from(unsafe { sys::negs_film_format(mm) }).ok()
}

/// A photograph's aspect ratio as people name it, such as 16:9.
pub fn ratio_name(width: f64, height: f64) -> String {
    words_from(|out, cap| unsafe { sys::negs_ratio_name(width, height, out, cap) })
}


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

    // Phase 4a

    fn c_words(p: *const *const c_char, n: usize) -> Vec<&'static str> {
        (0..n).map(|i| static_str(unsafe { *p.add(i) })).collect()
    }

    fn words(which: u32) -> Vec<&'static str> {
        let mut n = 0;
        let p = unsafe { sys::negs_state_words(which, &mut n) };
        c_words(p, n)
    }

    fn same_control(rust: &Adjustment, core: &sys::negs_adjustment_t) {
        let (title, path, help) = (static_str(core.title), static_str(core.path), static_str(core.help));
        let options = c_words(core.options, core.options_len);
        match *rust {
            Adjustment::Slider { title: t, path: p, low, high, step } => {
                assert_eq!((core.kind, title, path, help), (0, t, p, ""));
                assert_eq!((core.low.to_bits(), core.high.to_bits(), core.step.to_bits()), (low.to_bits(), high.to_bits(), step.to_bits()), "{p}");
            }
            Adjustment::Toggle { title: t, path: p } => assert_eq!((core.kind, title, path, help), (1, t, p, "")),
            Adjustment::Choice { title: t, path: p, options: o } => {
                assert_eq!((core.kind, title, path, help), (2, t, p, ""));
                assert_eq!(options, o);
            }
            Adjustment::BalancePrint { title: t, help: h } => assert_eq!((core.kind, title, path, help), (3, t, "", h)),
            Adjustment::FilterLink { help: h } => assert_eq!((core.kind, title, path, help), (4, "", "", h)),
        }
    }

    /// The Rust tables are the core's, field for field.
    #[test]
    fn tables_are_the_cores() {
        assert_eq!(unsafe { sys::negs_adjustment_group_count() }, ADJUSTMENT_GROUPS.len());
        for (i, g) in ADJUSTMENT_GROUPS.iter().enumerate() {
            let mut c: sys::negs_adjustment_group_t = unsafe { std::mem::zeroed() };
            assert_eq!(unsafe { sys::negs_adjustment_group(i, &mut c) }, 1);
            let enable = (!c.enable.is_null()).then(|| static_str(c.enable));
            assert_eq!((static_str(c.title), c.section, enable, c.basic), (g.title, g.section as u32, g.enable, g.basic));
            assert_eq!(c.controls_len, g.controls.len(), "{}", g.title);
            for (j, control) in g.controls.iter().enumerate() {
                same_control(control, unsafe { &*c.controls.add(j) });
            }
        }
        let loose: [&[Adjustment]; 8] =
            [PREP_TONE, PREP_COLOR, PREP_DETAIL, &[NEGATIVE_EXPOSURE], &[LENS_BLUR], &[WB_TEMPERATURE], &[WB_TINT], &[FILM_FORMAT]];
        for (which, list) in loose.iter().enumerate() {
            let mut n = 0;
            let p = unsafe { sys::negs_adjustments(which as u32, &mut n) };
            assert_eq!(n, list.len());
            for (j, control) in list.iter().enumerate() {
                same_control(control, unsafe { &*p.add(j) });
            }
        }
        assert_eq!(words(0), METERING);
        assert_eq!(words(1), INPUT_MODES);
        assert_eq!(words(2), ASSUMED_SPACES);
        assert_eq!(words(3), MANUAL_SPACES);
        assert_eq!(words(4), WHITE_BALANCE);
        assert_eq!(words(5), GRAIN_STOCK_FIELDS);
        assert_eq!(words(6), DIR_HALATION_STOCK_PATHS);
        let mut formats = vec![];
        let (mut name, mut mm) = (std::ptr::null(), 0.0);
        while unsafe { sys::negs_film_format_row(formats.len(), &mut name, &mut mm) } == 1 {
            formats.push((static_str(name), mm));
        }
        assert_eq!(formats, FILM_FORMATS);
        let constant = |i| static_str(unsafe { sys::negs_state_rules_constant(i) });
        assert_eq!(constant(0), NEGS_BUILT_HINT);
        assert_eq!((constant(3), constant(4), constant(5), constant(6)), (ColorFilter::PATH, ColorFilter::NONE, FilterLink::MAGENTA, FilterLink::YELLOW));
        let number = |i| unsafe { sys::negs_state_rules_number(i) };
        assert_eq!((number(0), number(1), number(2)), (PREP_EXPOSURE_RANGE.0, PREP_EXPOSURE_RANGE.1, PREP_EXPOSURE_STEP));
        assert_eq!((number(3), number(4)), FilterLink::RANGE);
        let enlarger = ADJUSTMENT_GROUPS.iter().flat_map(|g| g.controls).filter_map(|c| match c {
            Adjustment::BalancePrint { help, .. } | Adjustment::FilterLink { help } => Some(*help),
            _ => None,
        });
        assert_eq!(enlarger.collect::<Vec<_>>(), [constant(1), constant(2)]);
    }

    /// The same values as the macOS app's slider tests.
    #[test]
    fn snap_steps_and_clamps() {
        assert_eq!(snap(0.123456, 0.0, 2.0, 0.01), 0.12);
        assert_eq!(snap(2.7, 1.0, 4.0, 1.0), 3.0);
        assert_eq!(snap(99.0, -5.0, 5.0, 0.1), 5.0);
        assert_eq!(snap(0.0251, 0.0, 0.2, 0.001), 0.025);
        assert_eq!(snap(0.33, 0.1, 1.0, 0.05), 0.35);
        assert_eq!(snap(-1.234, -1.5, 1.5, 0.01), -1.23);
    }

    #[test]
    fn values_show_the_step_decimals() {
        assert_eq!(format_value(0.5, 0.01), "0.5");
        assert_eq!(format_value(-0.0001, 0.1), "0");
        assert_eq!(format_value(12.0, 1.0), "12");
        assert_eq!(format_value(0.025, 0.001), "0.025");
        assert_eq!(decimals(0.05), 2);
    }

    #[test]
    fn grain_lanes_follow_the_stock_preset() {
        assert_eq!(grain_lane("film_render.grain.rms_granularity.2"), Some(("rms_granularity", 2)));
        assert_eq!(grain_lane("film_render.grain.micro_structure.0"), None);
        assert_eq!(grain_lane("film_render.grain.blur"), None);
        assert!(color_only("film_render.grain.uniformity.1"));
        assert!(!color_only("film_render.grain.uniformity.0"));
        assert!(!color_only("film_render.grain.particle_scale_sublayers.2"));
        assert_eq!(bw_title("film_render.grain.uniformity.0"), Some("Uniformity"));
        assert_eq!(bw_title("film_render.grain.uniformity.1"), None);
        let lanes: Vec<_> = ADJUSTMENT_GROUPS.iter().flat_map(|g| g.controls).filter_map(|c| grain_lane(c.path())).collect();
        assert_eq!(lanes.len(), 9);
    }

    #[test]
    fn film_formats_read_back_from_the_long_edge() {
        let name = |mm: f64| film_format(mm).map(|i| FILM_FORMATS[i].0);
        assert_eq!(name(35.0), Some("35 mm (135)"));
        assert_eq!(name(56.0), Some("6×4.5 / 6×6"));
        assert_eq!(name(24.0f32 as f64), Some("Half frame"));
        assert_eq!((name(45.0), name(36.0)), (None, None));
    }

    fn stock(id: &str, name: &str, paper: bool, positive: bool, bw: bool, target: &str) -> Stock {
        Stock {
            id: id.into(),
            name: name.into(),
            paper,
            positive,
            bw,
            target: target.into(),
            times: vec![],
            grain: Value::Null,
            fotufilm: false,
            manufacturer_data: true,
            negs_built: false,
            cine_print: false,
        }
    }

    #[test]
    fn stocks_group_like_the_mac() {
        let films = [
            stock("gold", "Gold", false, false, false, "endura"),
            stock("vision", "Vision3", false, false, false, "print2383"),
            stock("velvia", "Velvia", false, true, false, ""),
            stock("trix", "Tri-X", false, false, true, ""),
        ];
        let papers = [stock("endura", "Endura", true, false, false, ""), stock("print2383", "2383 Print Film", true, false, false, "")];
        let (f, p): (Vec<&Stock>, Vec<&Stock>) = (films.iter().collect(), papers.iter().collect());
        let titles: Vec<_> = film_groups(&f, &p).iter().map(|g| g.title).collect();
        assert_eq!(titles, ["Color Negative", "Color Slide", "Motion Picture", "Black & White"]);
        let titles: Vec<_> = paper_groups(&p).iter().map(|g| (g.title, g.stocks.len())).collect();
        assert_eq!(titles, [("Color Paper", 1), ("Motion Picture Print", 1)]);
        assert_eq!(option_title("some_new_option"), "Some New Option");
        assert_eq!(option_title("center_weighted"), "Center-Weighted");
        assert_eq!(ratio_name(1920.0, 1080.0), "16:9");
        assert_eq!(ratio_name(1000.0, 1300.0), "1.30:1");
        // Without the host's `contains`, the search compares bytes.
        let ids = |query: &str| -> Vec<String> { matching(query, film_groups(&f, &p)).iter().flat_map(|g| g.stocks.iter().map(|s| s.id.clone())).collect() };
        assert_eq!(ids(" "), ["gold", "velvia", "vision", "trix"]);
        assert_eq!(ids("Slide"), ["velvia"]);
        assert_eq!(ids("Motion Vision3"), ["vision"]);
        assert!(ids("gold Slide").is_empty());
    }

    #[test]
    fn stocks_and_filters_read_the_catalog() {
        let v = json!({"id": "kodak_gold_200", "name": "Gold 200", "paper": false, "bw": false, "target_print": "kodak_portra_endura",
            "development_times": [6.5, 8], "grain": {"rms_granularity": [0.1, 0.2, 1e-7]}, "fotufilm": true, "manufacturer_data": false, "cine_print": true});
        let s = Stock::new(&v);
        assert_eq!((s.id.as_str(), s.name.as_str(), s.target.as_str(), s.times.as_slice()), ("kodak_gold_200", "Gold 200", "kodak_portra_endura", &[6.5, 8.0][..]));
        assert_eq!(s.grain, v["grain"]);
        assert!(s.fotufilm && !s.manufacturer_data && s.cine_print && !s.paper);
        assert!(Stock::new(&json!({})).manufacturer_data);
        assert_eq!(Stock::new(&json!(null)).grain, Value::Null);
        assert!(FotufilmFilms::Off.lists(&Stock::new(&json!({}))) && !FotufilmFilms::Datasheets.lists(&s));
        assert_eq!(FotufilmFilms::from_key("datasheets").title(), "From datasheets");
        let f = ColorFilter::new(&json!({"key": "hoya_y2", "name": "Yellow", "glass": "Hoya Y2", "colour": "#F0C000"}));
        assert_eq!(f.colour, 0xF0C000);
        assert_eq!(ColorFilter::new(&json!({"colour": "x"})).colour, 0x888888);
        assert_eq!(ColorFilter::hint(Some(&f), Some(1.26), "HP5", true, ','), format!("{} Takes 1,3 stops on HP5; auto exposure gives it.", ColorFilter::effect("hoya_y2")));
        assert_eq!(ColorFilter::help(None), "No filter in front of the lens.");
        assert_eq!(FilterLink::from_key("opposite").title(), "Opposite ways");
        assert_eq!(FilterLink::Together.shifts(FilterLink::MAGENTA, 10.0, 0.0, 45.0), (5.0, 50.0));
    }

    #[test]
    fn choosing_a_film_brings_its_paper_and_presets() {
        let stocks = [
            stock("gold", "Gold", false, false, false, "endura"),
            stock("velvia", "Velvia", false, true, false, "endura"),
            stock("endura", "Endura", true, false, false, ""),
            stock("supra", "Supra", true, false, false, ""),
        ];
        let state = EditState {
            params: json!({"film_render": {"development_time": 7, "grain": {"rms_granularity": [1, 2, 3]}}, "io": {"scan_film": false}}),
            film: "gold".into(),
            paper: "supra".into(),
            ..Default::default()
        };
        let next = choosing(&state, "velvia", false, false, &stocks);
        assert_eq!((next.film.as_str(), next.paper.as_str()), ("velvia", "endura"));
        assert_eq!(next.params["film_render"]["grain"]["rms_granularity"], Value::Null);
        assert_eq!(next.params["film_render"]["development_time"], Value::Null);
        assert_eq!(next.params["io"]["scan_film"], json!(true));
        assert_eq!(next.params["print_render"]["development_time"], Value::Null);
        let kept = choosing(&state, "velvia", false, true, &stocks);
        assert_eq!(kept.paper, "supra");
        let paper = choosing(&state, "endura", true, false, &stocks);
        assert_eq!((paper.film.as_str(), paper.paper.as_str()), ("gold", "endura"));
        let mut paired = state.clone();
        pair_film(&mut paired, &stocks, true);
        assert_eq!((paired.paper.as_str(), &paired.params["io"]["scan_film"]), ("endura", &json!(false)));
    }
}
