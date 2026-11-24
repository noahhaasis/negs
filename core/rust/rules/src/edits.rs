//! windows/src/edits.rs on the shared core (core/src/state/edits): what each
//! inspector tab holds (the counts beside the tabs, the pane header's "3
//! edits", the list a count opens) and what the Lab keeps back behind "N
//! more". The rules run in Odin.
//!
//! The model's facts stay where they are: edits.rs's functions take
//! anything that answers `Facts` (windows/src/edits.rs implements it for
//! `InspectorFacts`), and the core asks it through a negs_facts_host_t for
//! the call, so nothing is converted (the inspector asks per row, ~150
//! times a sync). A host keeping no state of its own gives the core the
//! state and the catalog instead (negs_facts_update, Phase 4d).
//!
//! Kept in Rust: the path tables (`const`s, checked against the core's),
//! `section_key`'s `format!` and `Format::display`'s match on the Value.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;

use negs_core_sys::state as sys;
use serde_json::Value;

use crate::crop::static_str;
use crate::state::{ADJUSTMENT_GROUPS, Section, borrowed, words_from};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tab {
    Prep,
    Lab,
    Output,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Prep, Tab::Lab, Tab::Output];

    pub fn title(self) -> &'static str {
        static_str(unsafe { sys::negs_edits_tab_title(self as u32) })
    }
}

/// What a count stands for, and what going back does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Param(&'static str),
    /// Prep › Match camera brightness, back to its setting for new photographs.
    MatchBrightness,
    /// Prep › Exposure, back to the match (or 0).
    Exposure,
    /// Lab › Scan film directly, back to what the film is (negative or slide).
    ScanFilm,
    /// Lab › Output › Colour space, back to the first of the spaces.
    PreviewSpace,
    Border,
    /// Camera › Film format: the value, and the Custom pick.
    FilmFormat,
}

impl Target {
    fn of(kind: u32, path: *const std::os::raw::c_char) -> Self {
        match kind {
            1 => Target::MatchBrightness,
            2 => Target::Exposure,
            3 => Target::ScanFilm,
            4 => Target::PreviewSpace,
            5 => Target::Border,
            6 => Target::FilmFormat,
            _ => Target::Param(static_str(path)),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Edit {
    pub title: String,
    pub value: String,
    pub target: Target,
}

pub const WHITE_BALANCE_PATHS: [&str; 3] = ["io.raw_white_balance.mode", "io.raw_white_balance.temperature", "io.raw_white_balance.tint"];
pub const INPUT_PATHS: [&str; 5] =
    ["io.input_interpretation.mode", "io.input_interpretation.color_space", "io.input_interpretation.linear", "io.input_color_space", "io.input_cctf_decoding"];
pub const FILM_DEVELOPMENT: &str = "film_render.development_time";
pub const PRINT_DEVELOPMENT: &str = "print_render.development_time";

/// The preferences the edit lists read (`Preferences`).
#[derive(Clone, Copy, Default, Debug)]
pub struct Prefs<'a> {
    /// View › Show All Controls.
    pub lab_all_controls: bool,
    /// The modules and sections whose "N more" was clicked, comma-separated.
    pub more_adjustments: &'a str,
    /// Match camera brightness for new photographs.
    pub match_new_photos: bool,
    /// The last save's colour space, the preview's.
    pub color_space: &'a str,
}

/// What edits.rs reads from the editor (inspector_view's `InspectorFacts`).
pub trait Facts {
    /// The value at `path`, a grain lane's preset when null.
    fn value(&self, path: &str) -> &Value;
    fn is_modified(&self, path: &str) -> bool;
    fn film_is_bw(&self) -> bool;
    /// The film (the first stock that isn't a paper with the state's film
    /// id) is a slide.
    fn film_positive(&self) -> bool;
    fn border_enabled(&self) -> bool;
    fn matches_brightness(&self) -> bool;
    fn exposure_offset(&self) -> f64;
    fn exposure_offset_modified(&self) -> bool;
    /// The catalog's filter in front of the lens, by name; None for none.
    fn chosen_color_filter_name(&self) -> Option<&str>;
    fn prefs(&self) -> Prefs<'_>;
    fn is_raw(&self) -> bool;
    fn lens_supported(&self) -> bool;
}

// MARK: The core asking the facts

struct Ask<'a, F> {
    facts: &'a F,
    flags: Cell<Option<u32>>,
    json: RefCell<Vec<u8>>,
}

unsafe fn ask<'a, F>(user: *mut c_void) -> &'a Ask<'a, F> {
    unsafe { &*(user as *const Ask<'a, F>) }
}

unsafe fn path<'a>(data: *const u8, len: usize) -> &'a str {
    unsafe { borrowed(sys::negs_str_t { data, len }) }
}

unsafe extern "C" fn is_modified<F: Facts>(user: *mut c_void, data: *const u8, len: usize) -> u8 {
    let a = unsafe { ask::<F>(user) };
    a.facts.is_modified(unsafe { path(data, len) }) as u8
}

unsafe extern "C" fn value<F: Facts>(user: *mut c_void, data: *const u8, len: usize, out: *mut u8, cap: usize) -> usize {
    let a = unsafe { ask::<F>(user) };
    let mut json = a.json.borrow_mut();
    json.clear();
    serde_json::to_writer(&mut *json, a.facts.value(unsafe { path(data, len) })).expect("a serde_json Value always writes");
    if !out.is_null() {
        let n = json.len().min(cap);
        unsafe { std::ptr::copy_nonoverlapping(json.as_ptr(), out, n) };
    }
    json.len()
}

unsafe extern "C" fn flags<F: Facts>(user: *mut c_void) -> u32 {
    let a = unsafe { ask::<F>(user) };
    if let Some(bits) = a.flags.get() {
        return bits;
    }
    let f = a.facts;
    let bits = [(f.film_is_bw(), sys::NEGS_FACTS_FILM_BW), (f.film_positive(), sys::NEGS_FACTS_FILM_POSITIVE), (f.border_enabled(), sys::NEGS_FACTS_BORDER), (f.matches_brightness(), sys::NEGS_FACTS_MATCHES)]
        .iter()
        .filter(|(on, _)| *on)
        .fold(0, |bits, (_, bit)| bits | bit);
    a.flags.set(Some(bits));
    bits
}

unsafe extern "C" fn exposure<F: Facts>(user: *mut c_void, out: *mut sys::negs_facts_exposure_t) {
    let a = unsafe { ask::<F>(user) };
    let modified = a.facts.exposure_offset_modified();
    // The offset only where a list shows it: it may work out the match's inputs.
    let offset = if modified { a.facts.exposure_offset() } else { 0.0 };
    unsafe { *out = sys::negs_facts_exposure_t { offset, modified: modified as u8 } };
}

unsafe extern "C" fn color_filter<F: Facts>(user: *mut c_void, name: *mut sys::negs_str_t) -> u8 {
    let a = unsafe { ask::<F>(user) };
    match a.facts.chosen_color_filter_name() {
        Some(n) => {
            unsafe { *name = sys::negs_str_t { data: n.as_ptr(), len: n.len() } };
            1
        }
        None => 0,
    }
}

struct Handle(Cell<*mut sys::negs_facts_t>);

impl Drop for Handle {
    fn drop(&mut self) {
        let h = self.0.replace(std::ptr::null_mut());
        if !h.is_null() {
            unsafe { sys::negs_facts_free(h) };
        }
    }
}

thread_local! {
    /// The thread's facts handle in host mode: its scratch is reused call
    /// after call.
    static HANDLE: Handle = const { Handle(Cell::new(std::ptr::null_mut())) };
}

/// Frees this thread's facts handle (the next call makes another); for
/// leak checks.
pub fn release_thread_facts() {
    HANDLE.with(|h| {
        let p = h.0.replace(std::ptr::null_mut());
        if !p.is_null() {
            unsafe { sys::negs_facts_free(p) };
        }
    });
}

fn s(text: &str) -> sys::negs_str_t {
    sys::negs_str_t { data: text.as_ptr(), len: text.len() }
}

/// `body` with the thread's facts handle asking `facts`.
fn asking<F: Facts, R>(facts: &F, body: impl FnOnce(*mut sys::negs_facts_t) -> R) -> R {
    let handle = HANDLE.with(|h| {
        if h.0.get().is_null() {
            h.0.set(unsafe { sys::negs_facts_new(std::ptr::null()) });
        }
        h.0.get()
    });
    let prefs = facts.prefs();
    let mut input: sys::negs_facts_input_t = unsafe { std::mem::zeroed() };
    input.prefs.lab_all_controls = prefs.lab_all_controls as u8;
    input.prefs.match_new_photos = prefs.match_new_photos as u8;
    input.prefs.more_adjustments = s(prefs.more_adjustments);
    input.prefs.color_space = s(prefs.color_space);
    input.is_raw = facts.is_raw() as u8;
    input.lens_supported = facts.lens_supported() as u8;
    let a = Ask { facts, flags: Cell::new(None), json: RefCell::new(Vec::new()) };
    let host = sys::negs_facts_host_t {
        user: &a as *const Ask<F> as *mut c_void,
        is_modified: is_modified::<F>,
        value: value::<F>,
        flags: flags::<F>,
        exposure: exposure::<F>,
        color_filter: color_filter::<F>,
    };
    unsafe {
        sys::negs_facts_update(handle, std::ptr::null(), &input, sys::NEGS_FACTS_BORROW);
        sys::negs_facts_set_host(handle, &host);
    }
    let result = body(handle);
    unsafe { sys::negs_facts_set_host(handle, std::ptr::null()) };
    result
}

// MARK: Counts and lists

/// A module's count: its switch, then its controls.
pub fn group_count<F: Facts>(index: usize, facts: &F) -> usize {
    let _ = &ADJUSTMENT_GROUPS[index];
    asking(facts, |h| unsafe { sys::negs_edits_group_count(h, index) })
}

pub fn scan_film_modified<F: Facts>(facts: &F) -> bool {
    asking(facts, |h| unsafe { sys::negs_edits_other_count(h, 2) } == 1)
}

/// The Lab's Output module: the preview's colour space and its curve.
pub fn output_count<F: Facts>(facts: &F) -> usize {
    asking(facts, |h| unsafe { sys::negs_edits_other_count(h, 1) })
}

pub fn section_count<F: Facts>(section: Section, facts: &F) -> usize {
    asking(facts, |h| unsafe { sys::negs_edits_section_count(h, section as u32) })
}

pub fn brightness_count<F: Facts>(facts: &F) -> usize {
    asking(facts, |h| unsafe { sys::negs_edits_other_count(h, 0) })
}

pub fn count<F: Facts>(tab: Tab, facts: &F) -> usize {
    asking(facts, |h| unsafe { sys::negs_edits_count(h, tab as u32) })
}

/// The controls on `tab` that are off their defaults, in the tab's order.
/// Always as many as `count(tab, facts)`.
pub fn edits<F: Facts>(tab: Tab, facts: &F, decimal: char) -> Vec<Edit> {
    asking(facts, |h| {
        let mut n = 0;
        let list = unsafe { sys::negs_edits_list(h, tab as u32, decimal as u32, &mut n) };
        (0..n)
            .map(|i| {
                let e = unsafe { &*list.add(i) };
                Edit { title: unsafe { borrowed(e.title) }.to_string(), value: unsafe { borrowed(e.value) }.to_string(), target: Target::of(e.target, e.path) }
            })
            .collect()
    })
}

/// "as_shot" as "As shot"; a path's last part likewise.
pub fn words(text: &str) -> String {
    words_from(|out, cap| unsafe { sys::negs_edits_words(text.as_ptr(), text.len(), out, cap) })
}

fn on_off(on: bool) -> String {
    if on { "On" } else { "Off" }.into()
}

/// The lists' numbers, with the locale's `decimal`.
pub struct Format {
    pub decimal: char,
}

impl Format {
    /// Up to two decimals, without grouping.
    pub fn number(&self, value: f64) -> String {
        words_from(|out, cap| unsafe { sys::negs_edits_number(value, self.decimal as u32, 0, out, cap) })
    }
    /// With its sign, except at zero.
    pub fn signed(&self, value: f64) -> String {
        words_from(|out, cap| unsafe { sys::negs_edits_number(value, self.decimal as u32, 1, out, cap) })
    }
    pub fn display(&self, value: &Value) -> String {
        match value {
            Value::Number(n) => self.number(n.as_f64().unwrap_or(0.0)),
            Value::Bool(b) => on_off(*b),
            Value::String(s) => words(s),
            _ => "Changed".into(),
        }
    }
}

// MARK: What the Lab shows (`InspectorReveal`)

/// The Lab keeps back each module's controls past its basics, and each
/// section's expert modules and rarer rows, behind "N more" (round 11, B).
/// View › Show All Controls shows everything; a control off its default
/// always shows.
pub fn section_title(section: Section) -> &'static str {
    static_str(unsafe { sys::negs_edits_section_title(section as u32) })
}

/// The `more_adjustments` key of a section's "N more".
pub fn section_key(section: Section) -> String {
    format!("section:{}", section_title(section))
}

/// Whether a section's extra modules and rows show.
pub fn shows_section<F: Facts>(facts: &F, section: Section) -> bool {
    asking(facts, |h| unsafe { sys::negs_edits_shows_section(h, section as u32) } == 1)
}

/// A module shows when it has basics, its section's extras show, or something in it changed.
pub fn group_shows<F: Facts>(facts: &F, index: usize) -> bool {
    let _ = &ADJUSTMENT_GROUPS[index];
    asking(facts, |h| unsafe { sys::negs_edits_group_shows(h, index) } == 1)
}

/// Whether a module's control at `place` shows: a basic, revealed, or changed.
pub fn control_shows<F: Facts>(facts: &F, index: usize, place: usize) -> bool {
    let group = &ADJUSTMENT_GROUPS[index];
    asking(facts, |h| {
        if place >= group.controls.len() {
            // edits.rs reads the control only when nothing before it says yes.
            let title = group.title;
            let shown = facts.prefs().lab_all_controls || unsafe { sys::negs_facts_revealed(h, title.as_ptr(), title.len()) } == 1 || place < group.basic;
            let _ = shown || group.controls[place].path().is_empty();
            return shown;
        }
        unsafe { sys::negs_edits_control_shows(h, index, place) == 1 }
    })
}

/// What a "N more" line does when clicked.
#[derive(Clone, PartialEq, Debug)]
pub enum MoreAction {
    Reveal(String),
    Conceal(String),
    /// Turns View › Show All Controls off.
    ShowFewer,
}

fn more(found: u8, line: &sys::negs_more_t) -> Option<(String, MoreAction)> {
    if found == 0 {
        return None;
    }
    let (text, key) = unsafe { (borrowed(line.text).to_string(), borrowed(line.key).to_string()) };
    let action = match line.action {
        0 => MoreAction::Reveal(key),
        1 => MoreAction::Conceal(key),
        _ => MoreAction::ShowFewer,
    };
    Some((text, action))
}

/// A module's "N more controls…", or once clicked "Hide N controls".
pub fn module_more<F: Facts>(facts: &F, index: usize) -> Option<(String, MoreAction)> {
    let _ = &ADJUSTMENT_GROUPS[index];
    asking(facts, |h| {
        let mut line: sys::negs_more_t = unsafe { std::mem::zeroed() };
        let found = unsafe { sys::negs_edits_module_more(h, index, &mut line) };
        more(found, &line)
    })
}

/// A section's "2 more: Scanner, Color Management…" for the modules and
/// rows it keeps back; once clicked, "Hide Scanner, Color Management".
pub fn section_more<F: Facts>(facts: &F, section: Section) -> Option<(String, MoreAction)> {
    asking(facts, |h| {
        let mut line: sys::negs_more_t = unsafe { std::mem::zeroed() };
        let found = unsafe { sys::negs_edits_section_more(h, section as u32, &mut line) };
        more(found, &line)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn paths(which: u32) -> Vec<&'static str> {
        let mut n = 0;
        let p = unsafe { sys::negs_edits_paths(which, &mut n) };
        (0..n).map(|i| static_str(unsafe { *p.add(i) })).collect()
    }

    #[test]
    fn tables_are_the_cores() {
        assert_eq!(paths(0), WHITE_BALANCE_PATHS);
        assert_eq!(paths(1), INPUT_PATHS);
        assert_eq!(paths(3), [FILM_DEVELOPMENT, PRINT_DEVELOPMENT]);
        assert_eq!(Tab::ALL.map(Tab::title), ["Prep", "Lab", "Output"]);
        assert_eq!(section_key(Section::Scan), "section:Scan & Color");
    }

    #[test]
    fn words_read_like_the_mac() {
        assert_eq!(words("as_shot"), "As shot");
        assert_eq!(words("io.input_interpretation.mode"), "Mode");
        assert_eq!(words(""), "");
    }

    #[test]
    fn numbers_keep_two_decimals_and_a_sign() {
        let f = Format { decimal: ',' };
        assert_eq!(f.number(1.5), "1,5");
        assert_eq!(f.number(2.0), "2");
        assert_eq!(f.signed(12.0), "+12");
        assert_eq!(f.signed(-0.25), "-0,25");
        assert_eq!(f.signed(0.0), "0");
        assert_eq!(f.display(&json!("as_shot")), "As shot");
        assert_eq!(f.display(&json!([1])), "Changed");
    }

    /// A small editor: edits off the defaults, nothing else.
    struct Small {
        params: Value,
        defaults: Value,
        more: String,
    }

    impl Facts for Small {
        fn value(&self, path: &str) -> &Value {
            crate::json::get(&self.params, path)
        }
        fn is_modified(&self, path: &str) -> bool {
            !crate::json::same(self.value(path), crate::json::get(&self.defaults, path))
        }
        fn film_is_bw(&self) -> bool {
            false
        }
        fn film_positive(&self) -> bool {
            false
        }
        fn border_enabled(&self) -> bool {
            true
        }
        fn matches_brightness(&self) -> bool {
            true
        }
        fn exposure_offset(&self) -> f64 {
            0.0
        }
        fn exposure_offset_modified(&self) -> bool {
            false
        }
        fn chosen_color_filter_name(&self) -> Option<&str> {
            Some("Yellow")
        }
        fn prefs(&self) -> Prefs<'_> {
            Prefs { more_adjustments: &self.more, match_new_photos: true, color_space: "sRGB", ..Default::default() }
        }
        fn is_raw(&self) -> bool {
            false
        }
        fn lens_supported(&self) -> bool {
            false
        }
    }

    #[test]
    fn counts_match_the_lists() {
        let defaults = json!({"prep": {"highlights": 0}, "camera": {"color_filter": "none", "lens_blur_um": 0}, "io": {"scan_film": false}});
        let mut small = Small { params: defaults.clone(), defaults, more: String::new() };
        crate::json::set(&mut small.params, "prep.highlights", json!(12));
        crate::json::set(&mut small.params, "camera.color_filter", json!("hoya_y2"));
        for tab in Tab::ALL {
            assert_eq!(edits(tab, &small, '.').len(), count(tab, &small), "{tab:?}");
        }
        assert_eq!(edits(Tab::Prep, &small, '.')[0], Edit { title: "Highlights".into(), value: "+12".into(), target: Target::Param("prep.highlights") });
        assert_eq!(edits(Tab::Lab, &small, '.')[0].value, "Yellow");
        assert_eq!(edits(Tab::Output, &small, '.')[0].target, Target::Border);
        let (text, _) = section_more(&small, Section::Camera).unwrap();
        assert!(text.contains("Lens blur"), "{text}");
        small.more = section_key(Section::Camera);
        assert!(shows_section(&small, Section::Camera));
        release_thread_facts();
    }
}
