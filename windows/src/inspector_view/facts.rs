//! What the inspector reads from the editor model, as plain data
//! (`Model::inspector_facts`). The model's pure readers live here and
//! `Model` delegates to them, so the inspector and the model can't disagree:
//! a value with its grain-lane default, whether it's modified, the colour
//! filter, the brightness match and the exposure above it, the preview's
//! colour space, the film format and the image size.
//!
//! Facts the model derives from its own caches (the negative's surveys and
//! bases, whether an engine is running) come computed.

use serde_json::Value;

use crate::crop::Size;
use crate::json;
use crate::negative::{self as neg, BaseStatus, FlatField};
use crate::state::{self, ColorFilter, EditState, FilterLink, SizeMode, Stock};

use super::output::COLOR_SPACES;

/// The edit and the catalog it's read against: values and their defaults.
#[derive(Clone, Copy)]
pub struct Params<'a> {
    pub state: &'a EditState,
    pub defaults: &'a Value,
    pub stocks: &'a [Stock],
}

impl<'a> Params<'a> {
    pub fn value(&self, path: &str) -> &'a Value {
        let value = self.state.value(path);
        match state::grain_lane(path) {
            Some((field, index)) if value.is_null() => self.grain_preset(field, index),
            _ => value,
        }
    }
    /// A grain preset lane defaults to the film's preset, not the engine's
    /// null.
    pub fn default_value(&self, path: &str) -> &'a Value {
        match state::grain_lane(path) {
            Some((field, index)) => self.grain_preset(field, index),
            None => json::get(self.defaults, path),
        }
    }
    pub fn film_stock(&self) -> Option<&'a Stock> {
        self.stocks.iter().find(|s| !s.paper && s.id == self.state.film)
    }
    pub fn film_is_bw(&self) -> bool {
        self.film_stock().is_some_and(|s| s.bw)
    }
    fn grain_preset(&self, field: &str, index: usize) -> &'a Value {
        static NULL: Value = Value::Null;
        self.film_stock().map_or(&NULL, |s| json::get(&s.grain, &format!("{field}.{index}")))
    }
    pub fn is_modified(&self, path: &str) -> bool {
        let (current, original) = (self.value(path), self.default_value(path));
        if let (Some(a), Some(b)) = (current.as_f64(), original.as_f64()) {
            // Rust defaults arrive as widened f32 values; slider edits are decimal.
            return (a - b).abs() > 1e-6 * b.abs().max(1.0);
        }
        !json::same(current, original)
    }
    pub fn size_mode(&self) -> SizeMode {
        state::size_mode(&self.state.params)
    }
    /// Whether the Lab shows the long-edge slider: Custom… was picked, or
    /// the length isn't one of the stills formats.
    pub fn film_format_custom(&self, custom: bool) -> bool {
        custom || state::film_format(json::number(self.value(state::FILM_FORMAT.path()))).is_none()
    }
    pub fn is_negative(&self) -> bool {
        neg::is_negative(self.state)
    }
}

/// The camera colour filters the catalog offers, and the one in front of
/// the lens.
#[derive(Clone, Copy)]
pub struct ColorFilters<'a> {
    pub params: Params<'a>,
    pub filters: &'a [ColorFilter],
    pub catalog: &'a Value,
}

impl<'a> ColorFilters<'a> {
    /// The filter in front of the lens, or `ColorFilter::NONE`.
    pub fn key(&self) -> &'a str {
        match self.params.value(ColorFilter::PATH).as_str() {
            Some(key) if !key.is_empty() => key,
            _ => ColorFilter::NONE,
        }
    }
    /// The catalog's filter in front of the lens; None for none.
    pub fn chosen(&self) -> Option<&'a ColorFilter> {
        let key = self.key();
        self.filters.iter().find(|f| f.key == key)
    }
    /// How many stops the filter takes on the current film; None for colour
    /// films and for a film the glass leaves blind.
    pub fn stops(&self, key: &str) -> Option<f64> {
        let film = &self.params.state.film;
        let profile = self.catalog["profiles"].as_array()?.iter().find(|p| p["id"].as_str() == Some(film.as_str()))?;
        profile["color_filter_stops"][key].as_f64()
    }
    /// The module shows for B&W films, or while a filter is set (by a
    /// preset, or kept from a B&W film), with a warning on colour film.
    pub fn shown(&self) -> bool {
        !self.filters.is_empty() && (self.params.film_is_bw() || self.key() != ColorFilter::NONE)
    }
}

/// Prep › Match camera brightness and the Exposure above it.
#[derive(Clone, Copy)]
pub struct Brightness<'a> {
    pub params: Params<'a>,
    pub is_raw: bool,
    /// Why the last match failed: the photograph's generation, the inputs
    /// it failed for (None: it can't run for this photograph), the message.
    pub failure: Option<&'a (u64, Option<Value>, String)>,
    /// The open photograph's generation.
    pub generation: u64,
    /// A solve is running.
    pub matching: bool,
}

impl Brightness<'_> {
    pub fn matches(&self) -> bool {
        self.params.state.brightness_match.is_some()
    }

    /// Why matching can't run for this photograph, or None.
    pub fn unavailable(&self) -> Option<String> {
        if let Some((generation, None, message)) = self.failure
            && *generation == self.generation
        {
            return Some(message.clone());
        }
        if !self.is_raw && json::string(self.params.value("io.input_interpretation.mode")) != "auto" {
            return Some("Matching reads the file’s own colour tags. Set Input Colour to Automatic to use it.".into());
        }
        None
    }

    pub fn failed_here(&self) -> bool {
        matches!(self.failure, Some((generation, Some(inputs), _))
            if *generation == self.generation && json::same(inputs, &state::match_inputs(self.params.state)))
    }

    /// On, possible, and not failed for the current settings: the meter then
    /// only sets where the highlight limit starts.
    pub fn active(&self) -> bool {
        self.matches() && self.unavailable().is_none() && !self.failed_here()
    }

    /// On and not yet solved for the current settings. Save and Export wait.
    pub fn pending(&self) -> bool {
        let Some(m) = &self.params.state.brightness_match else { return false };
        if !self.active() {
            return false;
        }
        m.ev.is_none() || !m.inputs.as_ref().is_some_and(|inputs| json::same(inputs, &state::match_inputs(self.params.state)))
    }

    /// The note under the Exposure slider.
    pub fn note(&self) -> Option<String> {
        if !self.matches() {
            return None;
        }
        if let Some(unavailable) = self.unavailable() {
            return Some(unavailable);
        }
        if self.failed_here()
            && let Some((_, _, message)) = self.failure
        {
            return Some(message.clone());
        }
        if self.pending() || self.matching {
            return Some("Matching…".into());
        }
        let m = self.params.state.brightness_match.as_ref()?;
        let ev = m.ev?;
        let value = format!("{}{:.2}", if ev >= 0.0 { "+" } else { "−" }, ev.abs());
        let reference = if self.is_raw { "the camera" } else { "the original" };
        Some(if m.highlight_limited {
            format!("Starts at {value} EV to match {reference}, held back to protect highlights.")
        } else {
            format!("Starts at {value} EV to match {reference}.")
        })
    }

    pub fn matched_ev(&self) -> f64 {
        if self.active() { self.params.state.brightness_match.as_ref().and_then(|m| m.ev).unwrap_or(0.0) } else { 0.0 }
    }

    /// The Exposure slider's range: the engine's total stays within ±3 EV.
    pub fn exposure_offset_range(&self) -> (f64, f64) {
        let matched = self.matched_ev();
        (state::PREP_EXPOSURE_RANGE.0 - matched, state::PREP_EXPOSURE_RANGE.1 - matched)
    }

    /// The Exposure slider: the user's exposure above the match.
    pub fn exposure_offset(&self) -> f64 {
        json::number(self.params.value(state::PREP_EXPOSURE)) - self.matched_ev()
    }

    pub fn exposure_offset_modified(&self) -> bool {
        (json::number(self.params.value(state::PREP_EXPOSURE)) - self.matched_ev()).abs() > 1e-6
    }

    /// Whether Prep shows Auto exposure and Metering: while matched the meter
    /// only sets where the highlight limit starts, so they show when matching
    /// is off or unavailable, or when changed.
    pub fn shows_meter(&self) -> bool {
        !self.active() || self.params.is_modified("camera.auto_exposure") || self.params.is_modified("camera.auto_exposure_method")
    }
}

/// The colour space the preview develops in: the one Save and Export last
/// used (`Preferences::color_space`), so editing shows the export's gamut.
pub fn preview_space(chosen: &str) -> &'static str {
    COLOR_SPACES.iter().find(|s| **s == chosen).copied().unwrap_or(COLOR_SPACES[0])
}

/// The frame and photograph `state` develops from a source of
/// `source_size`, uncapped, with the engine's resolver; None until the
/// source's size is known.
pub fn resolved_size(
    source_size: (u32, u32),
    state: &EditState,
) -> Option<Result<spektrafilm_io::image_size::ResolvedSize, spektrafilm_io::image_size::SizeError>> {
    (source_size.0 > 0).then(|| state::resolve_size(source_size, &state.params, &state.crop))
}

/// The exported photograph, cropped, before rotation, borders and export
/// reductions; zero until known.
pub fn photograph_size(source_size: (u32, u32), state: &EditState) -> Size {
    match resolved_size(source_size, state) {
        Some(Ok(size)) => Size::new(size.photo.0 as f64, size.photo.1 as f64),
        _ => Size::ZERO,
    }
}

/// The preferences the inspector reads (`Preferences`).
#[derive(Clone, Copy, Default)]
pub struct Prefs<'a> {
    /// View › Show All Controls.
    pub lab_all_controls: bool,
    /// The modules and sections whose "N more" was clicked, comma-separated.
    pub more_adjustments: &'a str,
    /// Match camera brightness for new photographs.
    pub match_new_photos: bool,
    /// The last save's choices, for the preview's space and the Export foot.
    pub color_space: &'a str,
    pub output_format: &'a str,
    pub jpeg_quality: u32,
    pub png_bit_depth: u32,
    pub tiff_bit_depth: u32,
    /// Enlarger › Shifts move.
    pub filter_shift_link: &'a str,
    /// The tone graph's channel.
    pub tone_graph_channel: &'a str,
}

impl Prefs<'_> {
    /// Whether a module's or section's "N more" was clicked
    /// (`Preferences::revealed`).
    #[allow(dead_code)] // The core reads the list itself (negs_facts_revealed).
    pub fn revealed(&self, key: &str) -> bool {
        self.more_adjustments.split(',').any(|t| t == key)
    }
    pub fn filter_link(&self) -> FilterLink {
        FilterLink::from_key(self.filter_shift_link)
    }
}

/// A negative's facts, from the model's surveys and bases.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NegativeFacts {
    /// Prep's Film base line (`Model::negative_sentence`).
    pub sentence: String,
    /// The scan doesn't look like a colour negative.
    pub warning: Option<&'static str>,
    /// Where the base came from; None while that isn't known.
    pub base: Option<BaseStatus>,
    /// This frame's light frame and whether it was used.
    pub flat_field: FlatField,
    /// A request about the negative is running.
    pub busy: bool,
    /// Prep › Backlight's "Backlight…" was clicked.
    pub backlight_shown: bool,
}

/// Everything the inspector's view and edit lists read from the model.
#[derive(Clone)]
pub struct InspectorFacts<'a> {
    pub params: Params<'a>,
    pub color_filters: ColorFilters<'a>,
    pub brightness: Brightness<'a>,
    pub prefs: Prefs<'a>,
    pub is_raw: bool,
    /// The RAW's decode found corrections for its lens.
    pub lens_supported: bool,
    /// A photograph is open.
    pub has_source: bool,
    /// The shown photograph's size after cropping, before rotation; zero
    /// before one opens (`Model::photo_size`).
    pub photo_size: Size,
    /// The source's pixels; (0, 0) until known.
    pub source_size: (u32, u32),
    /// The preview's pixels over the export's, and whether a preview shows.
    pub preview_scale: f64,
    pub image_shown: bool,
    /// Why the Longest side typed can't be used.
    pub size_error: Option<&'a str>,
    /// Lab › Camera › Film format › Custom… was picked.
    pub custom_film_format: bool,
    /// Balance Print is solving.
    pub balancing_print: bool,
    pub can_balance_print: bool,
    pub can_export: bool,
    pub cropping: bool,
    /// Some while the photograph is read as a negative.
    pub negative: Option<NegativeFacts>,
}

impl<'a> InspectorFacts<'a> {
    pub fn value(&self, path: &str) -> &'a Value {
        self.params.value(path)
    }
    pub fn default_value(&self, path: &str) -> &'a Value {
        self.params.default_value(path)
    }
    pub fn is_modified(&self, path: &str) -> bool {
        self.params.is_modified(path)
    }
    pub fn state(&self) -> &'a EditState {
        self.params.state
    }
    pub fn stocks(&self) -> &'a [Stock] {
        self.params.stocks
    }
    pub fn has_catalog(&self) -> bool {
        !self.params.stocks.is_empty()
    }
    pub fn film_is_bw(&self) -> bool {
        self.params.film_is_bw()
    }
    pub fn is_negative(&self) -> bool {
        self.params.is_negative()
    }
    pub fn matches_brightness(&self) -> bool {
        self.brightness.matches()
    }
    pub fn exposure_offset(&self) -> f64 {
        self.brightness.exposure_offset()
    }
    pub fn exposure_offset_modified(&self) -> bool {
        self.brightness.exposure_offset_modified()
    }
    pub fn exposure_offset_range(&self) -> (f64, f64) {
        self.brightness.exposure_offset_range()
    }
    pub fn shows_meter(&self) -> bool {
        self.brightness.shows_meter()
    }
    pub fn preview_space(&self) -> &'static str {
        preview_space(self.prefs.color_space)
    }
    pub fn color_filter(&self) -> &'a str {
        self.color_filters.key()
    }
    pub fn chosen_color_filter(&self) -> Option<&'a ColorFilter> {
        self.color_filters.chosen()
    }
    pub fn shows_color_filter(&self) -> bool {
        self.color_filters.shown()
    }
    pub fn size_mode(&self) -> SizeMode {
        self.params.size_mode()
    }
    pub fn film_format_custom(&self) -> bool {
        self.params.film_format_custom(self.custom_film_format)
    }
    pub fn filter_link(&self) -> FilterLink {
        self.prefs.filter_link()
    }
    pub fn resolved_size(&self) -> Option<Result<spektrafilm_io::image_size::ResolvedSize, spektrafilm_io::image_size::SizeError>> {
        resolved_size(self.source_size, self.params.state)
    }
    pub fn photograph_size(&self) -> Size {
        photograph_size(self.source_size, self.params.state)
    }
    /// The photograph's size in whole pixels; None before one opens.
    pub fn photo_pixels(&self) -> Option<(u32, u32)> {
        let size = self.photo_size;
        (!size.is_zero()).then(|| (size.width as u32, size.height as u32))
    }
    /// The development times the film's (or paper's) profile was measured at.
    pub fn stock_times(&self, paper: bool) -> Vec<f64> {
        let state = self.params.state;
        let id = if paper { &state.paper } else { &state.film };
        self.params.stocks.iter().find(|s| s.paper == paper && &s.id == id).map(|s| s.times.clone()).unwrap_or_default()
    }
    pub fn negative_busy(&self) -> bool {
        self.negative.as_ref().is_some_and(|n| n.busy)
    }
}
