//! The edit and the catalog: what the inspector shows and the engine renders.
//! Mirrors `Parameters.swift` and `Inspector.swift` in the macOS app.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::border::BorderSettings;
use crate::crop::CropSettings;
use crate::json;

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
}

impl Default for EditState {
    fn default() -> Self {
        Self {
            params: Value::Object(Map::new()),
            film: "kodak_gold_200".into(),
            paper: "kodak_portra_endura".into(),
            limit_preview: false,
            border: BorderSettings::default(),
            crop: CropSettings::default(),
            brightness_match: None,
        }
    }
}

impl PartialEq for EditState {
    fn eq(&self, other: &Self) -> bool {
        json::same(&self.params, &other.params)
            && self.film == other.film
            && self.paper == other.paper
            && self.limit_preview == other.limit_preview
            && self.border == other.border
            && self.crop == other.crop
            && self.brightness_match == other.brightness_match
    }
}

/// The exposure that matches the camera's brightness, solved live for the
/// settings it depends on (`BrightnessMatch` in `Parameters.swift`).
/// `prep.exposure_ev` holds the total the engine sees; the Exposure slider
/// shows and edits the part above `ev`.
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
/// the developed photograph is, except the brightness edits that are the
/// user's own (Prep, and the enlarger's print exposure) and settings that
/// can't change it. A change solves the match again; Prep edits never do.
pub fn match_inputs(state: &EditState) -> Value {
    let mut params = state.params.clone();
    for path in [
        "prep", "enlarger.print_exposure", "post", "film_render.grain", "io.rotate_cw", "io.crop", "io.crop_center",
        "io.crop_size", "io.output_color_space", "io.output_cctf_encoding", "settings.serial_grain",
    ] {
        json::set(&mut params, path, Value::Null);
    }
    serde_json::json!({"film": state.film, "paper": state.paper, "params": params})
}

impl EditState {
    /// The state without presentation-only settings, to decide whether an
    /// edit needs a new render. The preview is never rotated: the canvas
    /// turns it, and saving and exporting rotate the result. With an Image
    /// Size target the crop's extent sets the working size, so it stays; its
    /// position, angle and ratio never change the render.
    pub fn rendered(&self) -> EditState {
        let mut state = self.clone();
        state.border = BorderSettings::default();
        state.crop = CropSettings::default();
        state.brightness_match = None;
        if size_mode(&self.params) == SizeMode::LongestSide && self.crop.cropped() {
            state.crop.rect.width = self.crop.rect.width;
            state.crop.rect.height = self.crop.rect.height;
        }
        json::set(&mut state.params, "io.rotate_cw", Value::Null);
        state
    }

    pub fn value(&self, path: &str) -> &Value {
        json::get(&self.params, path)
    }

    pub fn rotation(&self) -> i32 {
        json::number(self.value("io.rotate_cw")) as i32
    }
}

/// Image Size (`ImageSize.swift`): the resolution the film develops at. A
/// Longest side target describes the cropped photograph before borders and
/// export reductions. The engine's older working-resolution factor stays at 1
/// in the app; restoring a session resets it.
pub const SIZE_TARGET: &str = "io.photo_long_side_px";
pub const SIZE_FACTOR: &str = "io.upscale_factor";

/// The camera's own lens corrections for a RAW (`io.lens_correction`), as on
/// macOS and Linux. Per photograph: on for each newly opened one and after
/// Reset, kept when a preset is applied (the engine's `presets::KEPT`).
pub const LENS_CORRECTION: &str = "io.lens_correction.enabled";

/// A saved edit's params on today's defaults. A session from before lens
/// corrections was developed without them, so they stay off and its crop
/// keeps its place.
pub fn restored_params(defaults: &Value, saved: &Value, nullable: &[String]) -> Value {
    let mut params = json::restoring(defaults, saved, nullable);
    if json::get(saved, LENS_CORRECTION).is_null() {
        json::set(&mut params, LENS_CORRECTION, Value::Bool(false));
    }
    params
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
    json::get(params, SIZE_TARGET).as_f64().map(|v| v as u32)
}

/// The frame `params` develops for a `source` of that size, and the
/// photograph `crop` cuts from it, with the engine's own resolver.
pub fn resolve_size(source: (u32, u32), params: &Value, crop: &CropSettings) -> Result<spektrafilm_io::image_size::ResolvedSize, spektrafilm_io::image_size::SizeError> {
    let engine = crop.cropped().then(|| crop.engine());
    spektrafilm_io::image_size::resolve(source, 1.0, size_target(params), engine.as_ref())
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
    let (field, index) = path.strip_prefix("film_render.grain.")?.split_once('.')?;
    GRAIN_STOCK_FIELDS.contains(&field).then_some(())?;
    Some((field, index.parse().ok()?))
}

/// B&W films have one emulsion: per-channel lanes past the first are hidden.
pub fn color_only(path: &str) -> bool {
    grain_lane(path).is_some_and(|(field, index)| field != "particle_scale_sublayers" && index > 0)
}

/// The first lane's title on a B&W film, without its colour.
pub fn bw_title(path: &str) -> Option<&'static str> {
    match path {
        "film_render.grain.rms_granularity.0" => Some("Granularity"),
        "film_render.grain.uniformity.0" => Some("Uniformity"),
        _ => None,
    }
}

impl Stock {
    pub fn new(value: &Value) -> Self {
        Self {
            id: json::string(&value["id"]).into(),
            name: json::string(&value["name"]).into(),
            paper: json::boolean(&value["paper"]),
            positive: json::boolean(&value["positive"]),
            bw: json::boolean(&value["bw"]),
            target: json::string(&value["target_print"]).into(),
            times: value["development_times"].as_array().map(|a| a.iter().map(json::number).collect()).unwrap_or_default(),
            grain: value["grain"].clone(),
        }
    }

    pub fn is_motion_picture_print(&self) -> bool {
        self.paper && (self.name.contains("Vision") || self.name.contains("Print Film"))
    }
}

/// The edit with stock `id` chosen, exactly as choosing it from the menu or
/// a preview tile makes it (`PreviewRules.choosing`). A film resets its
/// development time and its grain, DIR and halation presets, sets scanning
/// for slides and brings its own paper unless `keep_paper` (Settings › Keep
/// paper when changing film); a paper keeps the film.
pub fn choosing(state: &EditState, id: &str, paper: bool, keep_paper: bool, stocks: &[Stock]) -> EditState {
    let mut next = state.clone();
    if paper {
        next.paper = id.into();
        json::set(&mut next.params, "print_render.development_time", Value::Null);
        return next;
    }
    next.film = id.into();
    json::set(&mut next.params, "film_render.development_time", Value::Null);
    // A new film brings its own grain, DIR and halation presets, as upstream applies them.
    for field in GRAIN_STOCK_FIELDS {
        json::set(&mut next.params, &format!("film_render.grain.{field}"), Value::Null);
    }
    for path in DIR_HALATION_STOCK_PATHS {
        json::set(&mut next.params, path, Value::Null);
    }
    pair_film(&mut next, stocks, !keep_paper);
    next
}

/// Scanning for slides, and the film's own paper unless `paper` is false.
pub fn pair_film(state: &mut EditState, stocks: &[Stock], paper: bool) {
    let Some(film) = stocks.iter().find(|s| !s.paper && s.id == state.film) else { return };
    json::set(&mut state.params, "io.scan_film", Value::Bool(film.positive));
    if paper && stocks.iter().any(|s| s.paper && s.id == film.target) && state.paper != film.target {
        state.paper = film.target.clone();
        json::set(&mut state.params, "print_render.development_time", Value::Null);
    }
}

/// Stocks of one kind, in catalog order, for sectioned stock menus.
pub struct StockGroup<'a> {
    pub title: &'static str,
    pub stocks: Vec<&'a Stock>,
}

pub fn film_groups<'a>(films: &[&'a Stock], papers: &[&'a Stock]) -> Vec<StockGroup<'a>> {
    let cinema: Vec<&str> = papers.iter().filter(|p| p.is_motion_picture_print()).map(|p| p.id.as_str()).collect();
    group(films, &["Color Negative", "Color Slide", "Motion Picture", "Black & White"], |s| {
        if s.bw {
            "Black & White"
        } else if s.positive {
            "Color Slide"
        } else if cinema.contains(&s.target.as_str()) {
            "Motion Picture"
        } else {
            "Color Negative"
        }
    })
}

pub fn paper_groups<'a>(papers: &[&'a Stock]) -> Vec<StockGroup<'a>> {
    group(papers, &["Color Paper", "Motion Picture Print", "Black & White"], |s| {
        if s.bw {
            "Black & White"
        } else if s.is_motion_picture_print() {
            "Motion Picture Print"
        } else {
            "Color Paper"
        }
    })
}

/// The stocks matching every word of `query` in their name, id or group
/// title, ignoring case and accents ("portra 160", "kodak slide"); groups
/// left empty drop out (`StockGroup.matching`). An empty query keeps them all.
pub fn matching<'a>(query: &str, groups: Vec<StockGroup<'a>>) -> Vec<StockGroup<'a>> {
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return groups;
    }
    groups
        .into_iter()
        .filter_map(|group| {
            let stocks: Vec<&Stock> = group
                .stocks
                .into_iter()
                .filter(|s| {
                    let text = format!("{} {} {}", s.name, s.id.replace('_', " "), group.title);
                    words.iter().all(|word| contains_folded(&text, word))
                })
                .collect();
            (!stocks.is_empty()).then_some(StockGroup { title: group.title, stocks })
        })
        .collect()
}

/// Whether `text` contains `word`, ignoring case and diacritics, as
/// `localizedStandardContains` compares. The invariant locale keeps the
/// result the same on every machine; the catalog's names are English.
fn contains_folded(text: &str, word: &str) -> bool {
    use windows::Win32::Globalization::{FIND_FROMSTART, FindNLSStringEx, LINGUISTIC_IGNORECASE, LINGUISTIC_IGNOREDIACRITIC, LOCALE_NAME_INVARIANT};
    let (text, word): (Vec<u16>, Vec<u16>) = (text.encode_utf16().collect(), word.encode_utf16().collect());
    let flags = FIND_FROMSTART | LINGUISTIC_IGNORECASE.0 | LINGUISTIC_IGNOREDIACRITIC.0;
    unsafe { FindNLSStringEx(LOCALE_NAME_INVARIANT, flags, &text, &word, None, None, None, windows::Win32::Foundation::LPARAM(0)) >= 0 }
}

fn group<'a>(stocks: &[&'a Stock], order: &[&'static str], kind: impl Fn(&Stock) -> &'static str) -> Vec<StockGroup<'a>> {
    order
        .iter()
        .filter_map(|&title| {
            let stocks: Vec<&Stock> = stocks.iter().copied().filter(|s| kind(s) == title).collect();
            (!stocks.is_empty()).then_some(StockGroup { title, stocks })
        })
        .collect()
}

/// Human-readable names for engine option identifiers.
pub fn option_title(option: &str) -> String {
    let title = match option {
        "average" => "Average",
        "median" => "Median",
        "center_weighted" => "Center-Weighted",
        "partial" => "Partial",
        "matrix" => "Matrix",
        "multi_zone" => "Multi-Zone",
        "highlight_weighted" => "Highlight-Weighted",
        "as_shot" => "As Shot",
        "auto" => "Automatic",
        "daylight" => "Daylight",
        "tungsten" => "Tungsten",
        "custom" => "Custom",
        "black_pro_mist" => "Black Pro-Mist",
        "glimmerglass" => "Glimmerglass",
        "pro_mist" => "Pro-Mist",
        "cinebloom" => "CineBloom",
        "off" => "Off",
        "oklch" => "OKLCh",
        "oklrab" => "OKLrab",
        "cam16ucs" => "CAM16-UCS",
        "aces_rgc" => "ACES RGC",
        "xy" => "xy Chromaticity",
        "hanatos2025" => "Hanatos 2025",
        "arctic2026beta04" => "Arctic 2026 (Beta 04)",
        "mallett2019" => "Mallett 2019",
        _ => {
            return option
                .split('_')
                .map(|w| {
                    let mut c = w.chars();
                    c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" ");
        }
    };
    title.into()
}

/// Sliders move in steps; the engine expects the stepped value (integer
/// fields reject fractions). Like `EditorModel.snap`.
pub fn snap(value: f64, low: f64, high: f64, step: f64) -> f64 {
    let clamped = value.clamp(low, high);
    let steps = ((clamped - low) / step).round();
    let decimals = ((-step.log10()).floor() as i32 + 1).max(0);
    let scale = 10f64.powi(decimals);
    (((low + steps * step) * scale).round() / scale).min(high)
}

/// Decimal places a value with this step shows: as many as the step has
/// (0.05 shows two).
pub fn decimals(step: f64) -> usize {
    (0..6).find(|&d| {
        let scaled = step * 10f64.powi(d as i32);
        (scaled - scaled.round()).abs() < 1e-9
    }).unwrap_or(6)
}

/// A value as the inspector shows it: up to the step's decimals, no
/// trailing zeros.
pub fn format_value(value: f64, step: f64) -> String {
    let text = format!("{value:.*}", decimals(step));
    let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text };
    if text == "-0" { "0".into() } else { text }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Adjustment {
    Slider { title: &'static str, path: &'static str, low: f64, high: f64, step: f64 },
    Toggle { title: &'static str, path: &'static str },
    Choice { title: &'static str, path: &'static str, options: &'static [&'static str] },
    /// Solves the enlarger's M/Y filter shifts so a mid-grey prints neutral.
    BalancePrint { title: &'static str, help: &'static str },
}

impl Adjustment {
    /// The parameter a control edits; empty for the Balance Print button.
    pub fn path(&self) -> &'static str {
        match *self {
            Adjustment::Slider { path, .. } | Adjustment::Toggle { path, .. } | Adjustment::Choice { path, .. } => path,
            Adjustment::BalancePrint { .. } => "",
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
        basic: 4,
        controls: &[
            slider("Print exposure", "enlarger.print_exposure", 0.1, 5.0, 0.01),
            slider("Magenta shift", "enlarger.m_filter_shift", -50.0, 50.0, 0.1),
            slider("Yellow shift", "enlarger.y_filter_shift", -50.0, 50.0, 0.1),
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
    FILM_FORMATS.iter().position(|(_, edge)| (edge - mm).abs() < 0.05)
}

/// A photograph's aspect ratio as people name it, such as 16:9.
pub fn ratio_name(width: f64, height: f64) -> String {
    let ratio = width.max(height) / width.min(height);
    let named = [(1.0, "1:1"), (5.0 / 4.0, "5:4"), (7.0 / 5.0, "7:5"), (16.0 / 9.0, "16:9"), (2.0, "2:1"), (21.0 / 9.0, "21:9")];
    named
        .iter()
        .find(|(r, _)| (ratio / r - 1.0).abs() <= 0.01)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| format!("{ratio:.2}:1"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    use serde_json::json;

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

    #[test]
    fn grain_lanes_follow_the_stock_preset() {
        assert_eq!(grain_lane("film_render.grain.rms_granularity.2"), Some(("rms_granularity", 2)));
        assert_eq!(grain_lane("film_render.grain.micro_structure.0"), None);
        assert_eq!(grain_lane("film_render.grain.blur"), None);
        assert!(color_only("film_render.grain.uniformity.1"));
        assert!(!color_only("film_render.grain.uniformity.0"));
        assert!(!color_only("film_render.grain.particle_scale_sublayers.2"));
        let lanes: Vec<_> = ADJUSTMENT_GROUPS.iter().flat_map(|g| g.controls).filter_map(|c| grain_lane(c.path())).collect();
        assert_eq!(lanes.len(), 9);
    }

    /// `FilmFormatTests`: formats read back from the long edge.
    #[test]
    fn film_formats_read_back_from_the_long_edge() {
        let name = |mm: f64| film_format(mm).map(|i| FILM_FORMATS[i].0);
        assert_eq!(name(35.0), Some("35 mm (135)"), "The engine default is 135");
        assert_eq!(name(56.0), Some("6×4.5 / 6×6"));
        assert_eq!(name(120.0), Some("4×5"));
        assert_eq!(name(24.0f32 as f64), Some("Half frame"));
        assert_eq!((name(45.0), name(36.0)), (None, None));
        let Adjustment::Slider { low, high, .. } = FILM_FORMAT else { unreachable!() };
        assert!(FILM_FORMATS.iter().all(|(_, mm)| (low..=high).contains(mm)));
        let mut edges: Vec<i64> = FILM_FORMATS.iter().map(|(_, mm)| *mm as i64).collect();
        edges.dedup();
        assert_eq!(edges.len(), FILM_FORMATS.len());
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
    fn stocks_group_like_the_mac() {
        let stock = |id: &str, name: &str, paper: bool, positive: bool, bw: bool, target: &str| Stock {
            id: id.into(), name: name.into(), paper, positive, bw, target: target.into(), times: vec![], grain: Value::Null,
        };
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
        assert_eq!(ratio_name(1920.0, 1080.0), "16:9");
        assert_eq!(ratio_name(1000.0, 1300.0), "1.30:1");
    }

    /// `testFilmSearchMatchesEveryWordInNameIdOrGroup`, one case for one,
    /// then accents, ids and the order of the matches.
    #[test]
    fn film_search_matches_every_word_in_name_id_or_group() {
        let stock = |id: &str, name: &str, paper: bool, positive: bool, bw: bool| Stock {
            id: id.into(), name: name.into(), paper, positive, bw, target: String::new(), times: vec![], grain: Value::Null,
        };
        // The Mac's fixture: Gold 200, Velvia 50 and Tri-X 400.
        let films = [stock("gold", "Gold 200", false, false, false), stock("velvia", "Velvia 50", false, true, false), stock("trix", "Tri-X 400", false, false, true)];
        let f: Vec<&Stock> = films.iter().collect();
        let groups = || film_groups(&f, &[]);
        let ids = |query: &str| -> Vec<String> { matching(query, groups()).iter().flat_map(|g| g.stocks.iter().map(|s| s.id.clone())).collect() };
        assert_eq!(ids(""), ["gold", "velvia", "trix"]);
        assert_eq!(ids("  "), ["gold", "velvia", "trix"]);
        assert_eq!(ids("tri-x"), ["trix"]);
        assert_eq!(ids("GOLD 200"), ["gold"]);
        assert_eq!(ids("slide"), ["velvia"]);
        assert_eq!(ids("black 400"), ["trix"]);
        assert!(ids("gold 400").is_empty());
        assert_eq!(matching("velvia", groups()).iter().map(|g| g.title).collect::<Vec<_>>(), ["Color Slide"]);

        // Accents and case in the query or the name; any whitespace splits words.
        let more = [
            stock("kodak_portra_400", "Portra 400", false, false, false),
            stock("kodak_portra_160", "Portra 160", false, false, false),
            stock("fujifilm_velvia_100", "Velvia 100", false, true, false),
            stock("lomography_lomochrome_metropolis", "LomoChrome Métropolis", false, false, false),
            stock("kodak_trix400_d76_8min", "Tri-X 400 (D-76, 8 min)", false, false, true),
        ];
        let m: Vec<&Stock> = more.iter().collect();
        let ids = |query: &str| -> Vec<String> { matching(query, film_groups(&m, &[])).iter().flat_map(|g| g.stocks.iter().map(|s| s.id.clone())).collect() };
        assert_eq!(ids("PÓRTRÄ 400"), ["kodak_portra_400"]);
        assert_eq!(ids("metropolis"), ["lomography_lomochrome_metropolis"]);
        assert_eq!(ids("MÉTROPOLIS"), ["lomography_lomochrome_metropolis"]);
        assert_eq!(ids("portra\t160"), ["kodak_portra_160"]);
        // Words in either order; the matches keep the catalog's order.
        assert_eq!(ids("400 portra"), ids("portra 400"));
        assert_eq!(ids("portra"), ["kodak_portra_400", "kodak_portra_160"]);
        // The id with `_` read as spaces: "kodak" is only in the ids.
        assert_eq!(ids("kodak"), ["kodak_portra_400", "kodak_portra_160", "kodak_trix400_d76_8min"]);
        assert_eq!(ids("fujifilm velvia"), ["fujifilm_velvia_100"]);
        assert_eq!(ids("trix400 d76"), ["kodak_trix400_d76_8min"]);
        // An underscore in the query isn't a space, as on the Mac.
        assert!(ids("portra_400").is_empty());
        // Punctuation counts: "tri-x" isn't "trix", and a lone symbol matches only itself.
        assert_eq!(ids("tri-x"), ["kodak_trix400_d76_8min"]);
        assert!(ids("-x400").is_empty());
        // The group titles: Black & White, Color Slide, Color Negative.
        assert_eq!(ids("&"), ["kodak_trix400_d76_8min"]);
        assert_eq!(ids("color negative 400"), ["kodak_portra_400"]);
        assert!(ids("not-a-film").is_empty());
    }

    /// The helper's own catalog: every film finds itself. Skipped without a
    /// release helper.
    #[test]
    fn film_search_finds_the_live_catalog_films() {
        let Ok(paths) = crate::engine::EnginePaths::locate() else {
            eprintln!("skipped: build the release helper first");
            return;
        };
        let engine = crate::engine::EngineClient::start(paths).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        engine.request(json!({"command": "catalog"}), crate::engine::timeout_for("catalog"), move |result| {
            let _ = sender.send(result);
        });
        let catalog = receiver.recv().unwrap().unwrap().1;
        engine.stop();
        let stocks: Vec<Stock> = catalog["profiles"].as_array().map(|a| a.iter().map(Stock::new).collect()).unwrap_or_default();
        let films: Vec<&Stock> = stocks.iter().filter(|s| !s.paper).collect();
        let papers: Vec<&Stock> = stocks.iter().filter(|s| s.paper).collect();
        let ids = |query: &str| -> Vec<String> { matching(query, film_groups(&films, &papers)).iter().flat_map(|g| g.stocks.iter().map(|s| s.id.clone())).collect() };
        eprintln!("{} films in the live catalog", films.len());
        assert_eq!(ids("").len(), films.len());
        // Every film finds itself by its whole name, and by its id.
        for film in &films {
            assert!(ids(&film.name).contains(&film.id), "{}", film.name);
            assert!(ids(&film.id.replace('_', " ")).contains(&film.id), "{}", film.id);
        }
        let portra = ids("portra");
        assert!(!portra.is_empty() && portra.iter().all(|id| id.contains("portra")), "{portra:?}");
        assert!(ids("gold 400").is_empty());
        assert!(ids("slide").iter().all(|id| stocks.iter().any(|s| &s.id == id && s.positive)));
    }
}
