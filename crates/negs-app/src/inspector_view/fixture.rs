//! Tests: the facts of an editor without a `Model`, owned, with a fresh
//! model's defaults (`Model::new` with default preferences).

use serde_json::{Value, json};

use crate::crop::Size;
use crate::edits::Tab;
use crate::state::{ColorFilter, EditState, Stock};

use super::facts::{Brightness, ColorFilters, InspectorFacts, NegativeFacts, Params, Prefs};
use super::view::{Expanded, InspectorUi};

pub struct Fixture {
    pub state: EditState,
    pub defaults: Value,
    pub stocks: Vec<Stock>,
    pub filters: Vec<ColorFilter>,
    pub catalog: Value,
    pub lab_all_controls: bool,
    pub more_adjustments: String,
    pub match_new_photos: bool,
    pub color_space: String,
    pub output_format: String,
    pub jpeg_quality: u32,
    pub png_bit_depth: u32,
    pub tiff_bit_depth: u32,
    pub filter_shift_link: String,
    pub tone_graph_channel: String,
    pub is_raw: bool,
    pub lens_supported: bool,
    pub has_source: bool,
    pub photo_size: Size,
    pub source_size: (u32, u32),
    pub preview_scale: f64,
    pub image_shown: bool,
    pub size_error: Option<String>,
    pub custom_film_format: bool,
    pub balancing_print: bool,
    pub can_balance_print: bool,
    pub can_export: bool,
    pub cropping: bool,
    pub negative: Option<NegativeFacts>,
    pub failure: Option<(u64, Option<Value>, String)>,
    pub generation: u64,
    pub matching: bool,
    /// The inspector's own state.
    pub tab: Tab,
    pub expanded: String,
    pub all_open: bool,
    pub decimal: char,
}

pub fn stock(id: &str, name: &str, paper: bool, bw: bool) -> Stock {
    Stock {
        id: id.into(),
        name: name.into(),
        paper,
        positive: false,
        bw,
        target: String::new(),
        times: vec![],
        grain: Value::Null,
        fotufilm: false,
        manufacturer_data: true,
        negs_built: false,
        cine_print: false,
    }
}

impl Fixture {
    /// A model with a small catalog: the defaults the tests read, two
    /// films and a paper.
    pub fn new() -> Self {
        let defaults = json!({
            "camera": {"lens_blur_um": 0.0, "auto_exposure": true, "auto_exposure_method": "center_weighted", "exposure_compensation_ev": 0.0, "film_format_mm": 36.0},
            "scanner": {"lens_blur": 0.0},
            "enlarger": {"print_exposure": 1.0, "preflash_exposure": 0.0, "m_filter_shift": 0.0, "y_filter_shift": 0.0},
            "io": {"output_cctf_encoding": true, "scan_film": false, "input_interpretation": {"mode": "auto"}, "raw_white_balance": {"mode": "as_shot"}},
            "prep": {"exposure_ev": 0.0, "highlights": 0.0, "shadows": 0.0},
            "film_render": {"grain": {"active": true}},
        });
        let mut state = EditState::default();
        state.params = defaults.clone();
        state.film = "portra".into();
        state.paper = "endura".into();
        Self {
            state,
            defaults,
            stocks: vec![stock("portra", "Portra 400", false, false), stock("trix", "Tri-X 400", false, true), stock("endura", "Endura", true, false)],
            filters: vec![],
            catalog: Value::Null,
            lab_all_controls: false,
            more_adjustments: String::new(),
            match_new_photos: true,
            color_space: "sRGB".into(),
            output_format: "png".into(),
            jpeg_quality: 95,
            png_bit_depth: 8,
            tiff_bit_depth: 16,
            filter_shift_link: String::new(),
            tone_graph_channel: "rgb".into(),
            is_raw: false,
            lens_supported: false,
            has_source: true,
            photo_size: Size::ZERO,
            source_size: (0, 0),
            preview_scale: 1.0,
            image_shown: false,
            size_error: None,
            custom_film_format: false,
            balancing_print: false,
            can_balance_print: true,
            can_export: true,
            cropping: false,
            negative: None,
            failure: None,
            generation: 0,
            matching: false,
            tab: Tab::Prep,
            expanded: "Enlarger".into(),
            all_open: false,
            decimal: '.',
        }
    }

    pub fn facts(&self) -> InspectorFacts<'_> {
        let params = Params { state: &self.state, defaults: &self.defaults, stocks: &self.stocks };
        InspectorFacts {
            params,
            color_filters: ColorFilters { params, filters: &self.filters, catalog: &self.catalog },
            brightness: Brightness { params, is_raw: self.is_raw, failure: self.failure.as_ref(), generation: self.generation, matching: self.matching },
            prefs: Prefs {
                lab_all_controls: self.lab_all_controls,
                more_adjustments: &self.more_adjustments,
                match_new_photos: self.match_new_photos,
                color_space: &self.color_space,
                output_format: &self.output_format,
                jpeg_quality: self.jpeg_quality,
                png_bit_depth: self.png_bit_depth,
                tiff_bit_depth: self.tiff_bit_depth,
                filter_shift_link: &self.filter_shift_link,
                tone_graph_channel: &self.tone_graph_channel,
            },
            is_raw: self.is_raw,
            lens_supported: self.lens_supported,
            has_source: self.has_source,
            photo_size: self.photo_size,
            source_size: self.source_size,
            preview_scale: self.preview_scale,
            image_shown: self.image_shown,
            size_error: self.size_error.as_deref(),
            custom_film_format: self.custom_film_format,
            balancing_print: self.balancing_print,
            can_balance_print: self.can_balance_print,
            can_export: self.can_export,
            cropping: self.cropping,
            negative: self.negative.clone().filter(|_| crate::negative::is_negative(&self.state)),
        }
    }

    pub fn ui(&self) -> InspectorUi<'_> {
        InspectorUi { tab: self.tab, expanded: if self.all_open { Expanded::All } else { Expanded::Titles(&self.expanded) }, decimal: self.decimal }
    }

    pub fn view(&self) -> super::InspectorView {
        super::build(&self.facts(), &self.ui())
    }

    /// Sets a value of the edit.
    pub fn set(&mut self, path: &str, value: Value) {
        crate::json::set(&mut self.state.params, path, value);
    }

    /// Reveals a module's or section's "N more", as `Preferences::set_revealed`.
    pub fn reveal(&mut self, key: &str, shown: bool) {
        let mut keys: Vec<&str> = self.more_adjustments.split(',').filter(|t| !t.is_empty() && *t != key).collect();
        if shown {
            keys.push(key);
        }
        keys.sort();
        self.more_adjustments = keys.join(",");
    }
}
