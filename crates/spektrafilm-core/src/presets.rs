//! Presets: Python spektrafilm's saved GUI state (`spektrafilm_gui/persistence.py`
//! at experimental 28bf883), the format shared on pixls.us, read into and
//! written from Negs params. See `docs/presets-design.md`.
//!
//! Reading follows Python's loader: the file is merged onto the default GUI
//! state (`PROJECT_DEFAULT_GUI_STATE`, frozen in
//! `data/presets/spektrafilm-gui-default-28bf883.json` by
//! `scripts/parity/freeze_preset_default_state.py`), with unknown keys
//! ignored, missing ones kept and a non-list never replacing a list. The
//! state is then translated as `spektrafilm_gui/params_mapper.py` does.
//! Anything Negs can't render as Python would is refused with the reason.
//!
//! Python's `json.dump` writes `Infinity` and `NaN`, which JSON lacks. Only
//! `+Infinity` in a Langmuir knee means something (the linear law, `null` in
//! Negs params); any other non-finite number is refused.
use std::path::Path;
use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::params::RuntimeParams;
use crate::profile::{self, Profile};

const DEFAULT_GUI_STATE: &str = include_str!("../../../data/presets/spektrafilm-gui-default-28bf883.json");

const PRINT_ROUTE: &str = "input > film > print > scan";
const SCAN_ROUTE: &str = "input > film > scan";

/// Stand-ins for Python's non-finite numbers while the file is JSON.
const INFINITY: &str = "\u{1}Infinity";
const NEG_INFINITY: &str = "\u{1}-Infinity";
const NAN: &str = "\u{1}NaN";

/// GUI-state sections whose keys are the Negs group's: params_mapper copies
/// them whole.
const SECTIONS: [(&str, &str); 14] = [
    ("grain", "film_render.grain"),
    ("halation", "film_render.halation"),
    ("couplers", "film_render.dir_couplers"),
    ("chemistry", "print_render.density_curves_morph"),
    ("film_chemistry", "film_render.density_curves_morph"),
    ("film_base", "film_render.base"),
    ("print_base", "print_render.base"),
    ("camera", "camera"),
    ("camera_diffusion", "camera.diffusion_filter"),
    ("enlarger_diffusion", "enlarger.diffusion_filter"),
    ("glare", "print_render.glare"),
    ("scanner", "scanner"),
    ("input_gamut_compress", "io.input_gamut_compress"),
    ("output_gamut_compress", "io.output_gamut_compress"),
];

/// Single GUI-state values and their Negs paths, before [`SECTIONS`].
const FIELDS: [(&str, &str); 17] = [
    ("simulation.print_illuminant", "enlarger.illuminant"),
    ("simulation.print_exposure", "enlarger.print_exposure"),
    ("simulation.print_exposure_compensation", "enlarger.print_exposure_compensation"),
    ("simulation.print_y_filter_shift", "enlarger.y_filter_shift"),
    ("simulation.print_m_filter_shift", "enlarger.m_filter_shift"),
    ("preflashing.preflash_exposure", "enlarger.preflash_exposure"),
    ("preflashing.preflash_y_filter_shift", "enlarger.preflash_y_filter_shift"),
    ("preflashing.preflash_m_filter_shift", "enlarger.preflash_m_filter_shift"),
    ("chemistry.development_time", "print_render.development_time"),
    ("film_chemistry.development_time", "film_render.development_time"),
    ("input_image.rgb_to_raw_method", "settings.rgb_to_raw_method"),
    ("input_image.apply_hanatos2025_adaptation_window", "settings.apply_hanatos2025_adaptation_window"),
    ("input_image.apply_hanatos2025_adaptation_surface", "settings.apply_hanatos2025_adaptation_surface"),
    ("input_image.spectral_gaussian_blur", "settings.spectral_gaussian_blur"),
    ("load_raw.white_balance", "io.raw_white_balance.mode"),
    ("load_raw.temperature", "io.raw_white_balance.temperature"),
    ("load_raw.tint", "io.raw_white_balance.tint"),
];

/// Keys inside [`SECTIONS`] that aren't copied: unused upstream, overwritten
/// by `camera_diffusion` (params_mapper `_apply_camera`), or translated
/// separately (`active: false` is Negs's algorithm `"off"`).
const SKIPPED: [&str; 3] = ["grain.micro_sublayers", "camera.diffusion_filter", "input_gamut_compress.active"];

/// What a preset doesn't carry: applying one keeps the photograph's values.
/// The source's colour space and size, the output encoding, crop and
/// rotation, the speed and memory settings, CAT16 (a Negs-only switch) and
/// the Post tab. Every other params value comes from the preset or, where
/// Python's GUI has no such setting, Negs's default.
pub const KEPT: [&str; 20] = [
    "io.input_color_space",
    "io.input_cctf_decoding",
    "io.output_color_space",
    "io.output_cctf_encoding",
    "io.crop",
    "io.crop_center",
    "io.crop_size",
    "io.upscale_factor",
    "io.photo_long_side_px",
    "io.rotate_cw",
    "settings.use_enlarger_lut",
    "settings.use_scanner_lut",
    "settings.serial_grain",
    "settings.lut_resolution",
    "settings.use_fast_stats",
    "settings.preview_max_size",
    "settings.preview_mode",
    "settings.neutral_print_filters_from_database",
    "settings.use_cat16",
    "post",
];

/// A preset as Negs settings: the stocks and the params, with Negs's
/// defaults for [`KEPT`] (see [`apply`]).
#[derive(Debug, Clone)]
pub struct Preset {
    pub film: String,
    pub paper: String,
    pub params: RuntimeParams,
}

/// Read a preset file. Errors name the reason, for the user.
pub fn import(text: &str, data_dir: &Path) -> Result<Preset, String> {
    let state = gui_state(&parse(text)?)?;
    let (film, paper) = (stock(&state, "simulation.film_stock")?, stock(&state, "simulation.print_paper")?);
    let film_stock = load_stock(data_dir, &film, false)?;
    let paper_stock = load_stock(data_dir, &paper, true)?;
    let mut params = translate(&state)?;
    check(&state, &params)?;
    params.film_render.development_time = listed_time(&film_stock, params.film_render.development_time);
    params.print_render.development_time = listed_time(&paper_stock, params.print_render.development_time);
    Ok(Preset { film, paper, params })
}

/// A development time as the stock's family lists it: the entry the engine
/// picks anyway (the nearest), so the apps' pickers show it. A stock without
/// a family ignores the time; it becomes unset (Python writes 1.0 there).
fn listed_time(stock: &Profile, time: Option<f64>) -> Option<f64> {
    let times = &stock.data.development_time;
    if times.len() <= 1 {
        return None;
    }
    time.map(|t| times[profile::development_time_index(times, Some(t))])
}

/// The params after applying a preset: its own, with [`KEPT`] from `current`.
pub fn apply(preset: &RuntimeParams, current: &RuntimeParams) -> RuntimeParams {
    let mut params = serde_json::to_value(preset).expect("RuntimeParams serializes");
    let current = serde_json::to_value(current).expect("RuntimeParams serializes");
    for path in KEPT {
        let value = get(&current, path).expect("kept paths exist").clone();
        set(&mut params, path, value).expect("kept paths exist");
    }
    serde_json::from_value(params).expect("kept values come from RuntimeParams")
}

/// Write the settings as a preset file Python spektrafilm loads. A file can't
/// leave a value to the stock, so the stock presets the engine would use
/// (grain, DIR gammas, halation baseline, development time) are written out.
pub fn export(film_id: &str, paper_id: &str, params: &RuntimeParams, data_dir: &Path) -> Result<String, String> {
    let film = load_stock(data_dir, film_id, false)?;
    let paper = load_stock(data_dir, paper_id, true)?;
    let params = resolved(&film, &paper, params);
    // Through text, so f32 values are their shortest decimals (0.815, not
    // 0.8149999976158142) and read back to the same f32.
    let negs: Value = serde_json::from_str(&serde_json::to_string(&params).expect("RuntimeParams serializes"))
        .expect("serde_json reads its own output");
    let mut state = default_state().clone();
    let mut paths = Vec::new();
    leaves(&state, String::new(), &mut paths);
    let paths: Vec<String> = paths.into_iter().map(|(path, _)| path).collect();
    for gui in paths {
        if let Some(path) = negs_path(&gui) {
            let mut value = get(&negs, &path).cloned().ok_or_else(|| format!("no Negs value at {path}"))?;
            if is_langmuir(&path) {
                // Negs writes an infinite knee as null; Python's json.load reads Infinity.
                for k in value.as_array_mut().into_iter().flatten().filter(|k| k.is_null()) {
                    *k = Value::String(INFINITY.into());
                }
            }
            set(&mut state, &gui, value)?;
        }
    }
    let input_gamut = params.io.input_gamut_compress.algorithm != "off";
    for (path, value) in [
        ("simulation.film_stock", Value::from(film_id)),
        ("simulation.print_paper", Value::from(paper_id)),
        ("simulation.route", Value::from(if params.io.scan_film { SCAN_ROUTE } else { PRINT_ROUTE })),
        ("simulation.output_color_space", Value::from("sRGB")),
        ("input_gamut_compress.active", Value::from(input_gamut)),
        // Python's only input algorithm; `active` turns it off.
        ("input_gamut_compress.algorithm", Value::from("xy")),
        // Unused by Python (camera_diffusion wins), kept consistent.
        ("camera.diffusion_filter", get(&state, "camera_diffusion").cloned().unwrap_or_default()),
    ] {
        set(&mut state, path, value)?;
    }
    let text = serde_json::to_string_pretty(&state).expect("JSON values serialize");
    let marker = serde_json::to_string(INFINITY).expect("strings serialize");
    Ok(text.replace(&marker, "Infinity") + "\n")
}

/// `params` with every value the engine takes from the stocks when unset.
fn resolved(film: &Profile, paper: &Profile, params: &RuntimeParams) -> RuntimeParams {
    let mut params = params.clone();
    let mut stock = params.clone();
    crate::pipeline::apply_film_specific_params(film, &mut stock);
    let (dir, from) = (&mut params.film_render.dir_couplers, &stock.film_render.dir_couplers);
    dir.gamma_samelayer_rgb.get_or_insert(from.gamma_samelayer());
    dir.gamma_interlayer_r_to_gb.get_or_insert(from.gamma_r_to_gb());
    dir.gamma_interlayer_g_to_rb.get_or_insert(from.gamma_g_to_rb());
    dir.gamma_interlayer_b_to_rg.get_or_insert(from.gamma_b_to_rg());
    let (halation, from) = (&mut params.film_render.halation, &stock.film_render.halation);
    halation.halation_strength.get_or_insert(from.strength());
    halation.halation_first_sigma_um.get_or_insert(from.first_sigma_um());
    let stock_grain = crate::grain::grain_settings(film, &params.film_render.grain);
    let grain = &mut params.film_render.grain;
    grain.rms_granularity.get_or_insert(stock_grain.rms_granularity);
    grain.density_min.get_or_insert(stock_grain.density_min);
    grain.uniformity.get_or_insert(stock_grain.uniformity);
    grain.particle_scale_sublayers.get_or_insert(stock_grain.particle_scale_sublayers);
    // A family's default time; a colour stock has none and stays null.
    let default_time = |stock: &Profile| {
        let times = &stock.data.development_time;
        (times.len() > 1).then(|| times[profile::development_time_index(times, None)])
    };
    params.film_render.development_time = params.film_render.development_time.or_else(|| default_time(film));
    params.print_render.development_time = params.print_render.development_time.or_else(|| default_time(paper));
    params
}

/// The Negs path a GUI-state value maps to, `None` for values Negs doesn't
/// take from a preset (the source image, film-convert, display, saving).
fn negs_path(gui: &str) -> Option<String> {
    if let Some((_, path)) = FIELDS.iter().find(|(field, _)| *field == gui) {
        return Some(path.to_string());
    }
    if SKIPPED.iter().any(|skipped| gui.strip_prefix(skipped).is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))) {
        return None;
    }
    SECTIONS.iter().find_map(|(section, group)| {
        let key = gui.strip_prefix(section)?.strip_prefix('.')?;
        Some(format!("{group}.{key}"))
    })
}

fn is_langmuir(path: &str) -> bool {
    matches!(
        path,
        "film_render.dir_couplers.langmuir_donor_k_rgb" | "film_render.dir_couplers.langmuir_receiver_k_rgb"
    )
}

/// JSON with Python's `Infinity`, `-Infinity` and `NaN` tokens read as
/// marker strings. A UTF-8 byte-order mark is skipped.
fn parse(text: &str) -> Result<Value, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut json = String::with_capacity(text.len());
    let (mut in_string, mut escaped) = (false, false);
    let mut rest = text;
    'scan: while let Some(c) = rest.chars().next() {
        if in_string {
            (in_string, escaped) = (escaped || c != '"', !escaped && c == '\\');
        } else if c == '"' {
            in_string = true;
        } else {
            for (token, marker) in [("-Infinity", NEG_INFINITY), ("Infinity", INFINITY), ("NaN", NAN)] {
                if let Some(after) = rest.strip_prefix(token) {
                    json.push_str(&serde_json::to_string(marker).expect("strings serialize"));
                    rest = after;
                    continue 'scan;
                }
            }
        }
        json.push(c);
        rest = &rest[c.len_utf8()..];
    }
    serde_json::from_str(&json).map_err(|e| format!("The file isn't valid JSON ({e})."))
}

fn default_state() -> &'static Value {
    static STATE: OnceLock<Value> = OnceLock::new();
    STATE.get_or_init(|| serde_json::from_str(DEFAULT_GUI_STATE).expect("the frozen GUI state is JSON"))
}

/// The file merged onto the default GUI state as Python's loader does
/// (`persistence.gui_state_from_dict`), in the saved file's shape.
fn gui_state(file: &Value) -> Result<Value, String> {
    let Value::Object(file) = file else {
        return Err("The file isn't a spektrafilm preset: it holds no settings.".into());
    };
    let mut file = file.clone();
    if let Some(section) = file.get("input_image") {
        file.insert("input_image".into(), saved_input_image(section));
    }
    if let Some(section) = file.get("simulation") {
        file.insert("simulation".into(), saved_simulation(section));
    }
    // A top-level load_raw replaces gui_only's.
    if !file.get("load_raw").is_some_and(Value::is_object)
        && let Some(load_raw) = file.get("gui_only").and_then(|g| g.get("load_raw")).filter(|v| v.is_object())
    {
        file.insert("load_raw".into(), load_raw.clone());
    }
    let mut state = default_state().clone();
    merge(&mut state, &Value::Object(file));
    Ok(state)
}

/// Python's `_merge_into_dataclass`: only the target's keys; an object
/// merges into an object; only a list replaces a list (tuple); anything
/// replaces a scalar or null.
fn merge(target: &mut Value, data: &Value) {
    match target {
        Value::Object(fields) => {
            if let Value::Object(data) = data {
                for (key, field) in fields.iter_mut() {
                    if let Some(value) = data.get(key) {
                        merge(field, value);
                    }
                }
            }
        }
        Value::Array(_) => {
            if data.is_array() {
                *target = data.clone();
            }
        }
        _ => *target = data.clone(),
    }
}

/// `state.normalize_input_image_dict` in reverse: the saved, flat form, with
/// the legacy name of the upsampling method.
fn saved_input_image(section: &Value) -> Value {
    let Value::Object(data) = section else { return section.clone() };
    if data.contains_key("io") || data.contains_key("settings") {
        let mut flat = Map::new();
        for group in ["io", "settings"] {
            if let Some(Value::Object(values)) = data.get(group) {
                flat.extend(values.clone());
            }
        }
        return Value::Object(flat);
    }
    let mut flat = data.clone();
    if !flat.contains_key("rgb_to_raw_method")
        && let Some(method) = data.get("spectral_upsampling_method")
    {
        flat.insert("rgb_to_raw_method".into(), method.clone());
    }
    Value::Object(flat)
}

/// `state.normalize_simulation_dict`, in the saved, flat form: the dataclass
/// form flattened, and a missing or empty route taken from the legacy
/// `scan_film`, else the default.
fn saved_simulation(section: &Value) -> Value {
    let Value::Object(data) = section else { return section.clone() };
    if ["enlarger", "io", "workflow"].iter().any(|key| data.contains_key(*key)) {
        let mut flat = Map::new();
        for (key, path) in [
            ("film_stock", "selection.film_stock"),
            ("print_paper", "selection.print_paper"),
            ("print_illuminant", "enlarger.illuminant"),
            ("print_exposure", "enlarger.print_exposure"),
            ("print_exposure_compensation", "enlarger.print_exposure_compensation"),
            ("print_y_filter_shift", "enlarger.y_filter_shift"),
            ("print_m_filter_shift", "enlarger.m_filter_shift"),
            ("output_color_space", "io.output_color_space"),
            ("route", "route"),
        ] {
            if let Some(value) = get(section, path) {
                flat.insert(key.into(), value.clone());
            }
        }
        return Value::Object(flat);
    }
    let mut flat = data.clone();
    if !data.get("route").is_some_and(truthy) {
        if data.get("scan_film").is_some_and(truthy) {
            flat.insert("route".into(), SCAN_ROUTE.into());
        } else {
            flat.remove("route");
        }
    }
    Value::Object(flat)
}

/// Python's truthiness.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// The GUI state as Negs params, type-checked value by value.
fn translate(state: &Value) -> Result<RuntimeParams, String> {
    let defaults = serde_json::to_value(RuntimeParams::default()).expect("RuntimeParams serializes");
    let mut params = defaults.clone();
    let mut values = Vec::new();
    leaves(state, String::new(), &mut values);
    for (gui, value) in values {
        let Some(path) = negs_path(&gui) else { continue };
        let value = finite(&gui, &path, value)?;
        // Alone on the defaults, so a refusal names the value.
        let mut probe = defaults.clone();
        set(&mut probe, &path, value.clone())?;
        serde_json::from_value::<RuntimeParams>(probe)
            .map_err(|e| format!("“{gui}” has a value of the wrong kind ({e})."))?;
        set(&mut params, &path, value)?;
    }
    match get(state, "input_gamut_compress.active") {
        Some(Value::Bool(true)) => {}
        Some(Value::Bool(false)) => set(&mut params, "io.input_gamut_compress.algorithm", "off".into())?,
        _ => return Err("“input_gamut_compress.active” must be true or false.".into()),
    }
    let scan_film = match get(state, "simulation.route").and_then(Value::as_str) {
        Some(PRINT_ROUTE) => false,
        Some(SCAN_ROUTE) => true,
        _ => return Err("Negs renders a preset printed or as a film scan; this one uses another route.".into()),
    };
    set(&mut params, "io.scan_film", scan_film.into())?;
    serde_json::from_value(params).map_err(|e| format!("The preset's settings don't fit Negs ({e})."))
}

/// `value` with its non-finite markers resolved: `+Infinity` in a Langmuir
/// knee is `null`, any other is refused.
fn finite(gui: &str, path: &str, value: &Value) -> Result<Value, String> {
    let non_finite = |v: &Value| v.as_str().filter(|s| [INFINITY, NEG_INFINITY, NAN].contains(s)).map(str::to_owned);
    match value {
        Value::Array(items) if is_langmuir(path) => Ok(Value::Array(
            items
                .iter()
                .map(|item| match non_finite(item) {
                    Some(marker) if marker == INFINITY => Ok(Value::Null),
                    Some(marker) => Err(non_finite_error(gui, &marker)),
                    None => Ok(item.clone()),
                })
                .collect::<Result<_, _>>()?,
        )),
        Value::Array(items) => match items.iter().find_map(non_finite) {
            Some(marker) => Err(non_finite_error(gui, &marker)),
            None => Ok(value.clone()),
        },
        _ => match non_finite(value) {
            Some(marker) => Err(non_finite_error(gui, &marker)),
            None => Ok(value.clone()),
        },
    }
}

fn non_finite_error(gui: &str, marker: &str) -> String {
    format!("“{gui}” is {}, which Negs can't use.", &marker[1..])
}

/// What Python checks while loading or rendering and Negs can't follow.
fn check(state: &Value, params: &RuntimeParams) -> Result<(), String> {
    for swap in ["special.film_channel_swap", "special.print_channel_swap"] {
        let identity = get(state, swap)
            .and_then(Value::as_array)
            .is_some_and(|order| order.len() == 3 && order.iter().zip(0..).all(|(c, i)| c.as_f64() == Some(i as f64)));
        if !identity {
            return Err("The preset swaps colour channels, which Negs can't do.".into());
        }
    }
    let output = get(state, "simulation.output_color_space").and_then(Value::as_str).unwrap_or_default();
    if output != "sRGB" {
        return Err(format!("The preset renders into {output}; Negs reads presets made in sRGB only."));
    }
    let input_algorithm = get(state, "input_gamut_compress.algorithm").and_then(Value::as_str).unwrap_or_default();
    if input_algorithm != "xy" {
        return Err(format!("Unknown input gamut compression “{input_algorithm}”."));
    }
    crate::input_gamut::InputGamutCompress::from_params(&params.io.input_gamut_compress)
        .map_err(|e| format!("Input gamut compression: {e}."))?;
    check_output_gamut(&params.io.output_gamut_compress)?;
    crate::illuminants::standard_illuminant_f64(&params.enlarger.illuminant)
        .map_err(|e| format!("Enlarger light: {e}."))?;
    let filter = &params.camera.color_filter;
    if filter != crate::color_filters::NO_COLOR_FILTER
        && !crate::color_filters::COLOR_FILTER_LIBRARY.iter().any(|spec| spec.key == filter)
    {
        return Err(format!("Unknown camera colour filter “{filter}”."));
    }
    let metering = &params.camera.auto_exposure_method;
    if !crate::stages::filming::METERING_METHODS.contains(&metering.as_str()) {
        return Err(format!("Unknown auto-exposure metering “{metering}”."));
    }
    for family in [&params.camera.diffusion_filter.filter_family, &params.enlarger.diffusion_filter.filter_family] {
        if !spektrafilm_model::diffusion::is_known_family(family) {
            return Err(format!("Unknown diffusion filter “{family}”."));
        }
    }
    crate::spectral_service::upsampling_method(&params.settings.rgb_to_raw_method)
        .map_err(|_| format!("Unknown spectral upsampling “{}”.", params.settings.rgb_to_raw_method))?;
    params.io.raw_white_balance.validate().map_err(|e| format!("{e}."))?;
    Ok(())
}

/// Python `OutputGamutCompressSpec.__post_init__`, less `jzazbz`, which Negs
/// doesn't have.
fn check_output_gamut(gamut: &crate::params::OutputGamutCompressParams) -> Result<(), String> {
    match gamut.algorithm.as_str() {
        "off" | "aces_rgc" | "oklch" | "oklrab" | "cam16ucs" => {}
        "jzazbz" => return Err("The preset uses jzazbz output gamut compression, which Negs doesn't have.".into()),
        other => return Err(format!("Unknown output gamut compression “{other}”.")),
    }
    for (what, knee) in [("knee", Some(gamut.knee)), ("lightness compression", gamut.lightness_compression)] {
        let Some([threshold, limit, power]) = knee else { continue };
        let valid = (0.0..1.0).contains(&threshold) && limit > 0.0 && power > 0.0;
        if !valid {
            return Err(format!("Output gamut compression: invalid {what} {knee:?}.", knee = [threshold, limit, power]));
        }
    }
    Ok(())
}

fn stock(state: &Value, path: &str) -> Result<String, String> {
    match get(state, path) {
        Some(Value::String(id)) => Ok(id.clone()),
        _ => Err(format!("“{path}” isn't a stock name.")),
    }
}

/// The stock `id`, checked to be a paper (or print film) or a camera film.
fn load_stock(data_dir: &Path, id: &str, paper: bool) -> Result<Profile, String> {
    let what = if paper { "print paper" } else { "film" };
    let known = !id.is_empty()
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && data_dir.join("profiles").join(format!("{id}.json")).is_file();
    if !known {
        return Err(format!("Negs doesn't have the {what} “{id}”."));
    }
    let stock = profile::load_profile_by_name(data_dir, id).map_err(|e| format!("The {what} “{id}”: {e}"))?;
    if (stock.is_paper() || stock.is_printing()) != paper {
        return Err(format!("“{id}” isn't a {what}."));
    }
    Ok(stock)
}

/// Every non-object value with its dotted path.
fn leaves<'a>(value: &'a Value, path: String, out: &mut Vec<(String, &'a Value)>) {
    match value {
        Value::Object(fields) => {
            for (key, field) in fields {
                let path = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                leaves(field, path, out);
            }
        }
        _ => out.push((path, value)),
    }
}

fn get<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(value, |value, key| value.get(key))
}

/// Replace the value at an existing path.
fn set(value: &mut Value, path: &str, new: Value) -> Result<(), String> {
    let slot = path
        .split('.')
        .try_fold(value, |value, key| value.get_mut(key))
        .ok_or_else(|| format!("no value at {path}"))?;
    *slot = new;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    /// Negs values a preset sets to Negs's default: Python's GUI has no such
    /// setting and renders with the same default.
    const NEGS_DEFAULTS: [&str; 8] = [
        "film_render.grain.monochrome",
        "film_render.glare",
        "enlarger.normalize_print_exposure",
        "enlarger.y_filter_neutral",
        "enlarger.m_filter_neutral",
        "enlarger.c_filter_neutral",
        "enlarger.lens_blur",
        "io.input_gamut_compress.center",
    ];

    #[test]
    fn every_negs_value_is_mapped_kept_or_defaulted() {
        let mut gui = Vec::new();
        leaves(default_state(), String::new(), &mut gui);
        let mut mapped: Vec<String> = gui.iter().filter_map(|(path, _)| negs_path(path)).collect();
        mapped.extend(["io.scan_film".into(), "io.input_gamut_compress.algorithm".into()]);
        let defaults = serde_json::to_value(RuntimeParams::default()).unwrap();
        for path in &mapped {
            assert!(get(&defaults, path).is_some(), "{path} has no Negs field");
        }
        let mut negs = Vec::new();
        leaves(&defaults, String::new(), &mut negs);
        let under = |path: &str, prefix: &str| path == prefix || path.starts_with(&format!("{prefix}."));
        for (path, _) in negs {
            let homes = [mapped.iter().any(|m| *m == path), KEPT.iter().any(|k| under(&path, k)),
                NEGS_DEFAULTS.iter().any(|d| under(&path, d))];
            assert_eq!(homes.iter().filter(|h| **h).count(), 1, "{path}: mapped, kept, defaulted = {homes:?}");
        }
    }

    #[test]
    fn the_default_state_is_python_s_gold_200_supra_endura() {
        let preset = import(DEFAULT_GUI_STATE, &data_dir()).unwrap();
        assert_eq!((preset.film.as_str(), preset.paper.as_str()), ("kodak_gold_200", "kodak_supra_endura"));
        let p = &preset.params;
        assert_eq!(p.io.raw_white_balance.mode, spektrafilm_io::RawWhiteBalanceMode::Daylight);
        assert_eq!(p.film_render.dir_couplers.gamma_samelayer_rgb, Some([0.5159, 0.5934, 0.2829]));
        assert_eq!(p.film_render.grain.rms_granularity, Some([5.0; 3]));
        assert_eq!(p.film_render.development_time, None);
        assert_eq!(p.film_render.dir_couplers.langmuir_donor_k_rgb, [1.0; 3]);
        assert_eq!(p.io.input_gamut_compress.algorithm, "xy");
        assert!(!p.io.scan_film);
    }

    #[test]
    fn a_sparse_file_means_what_it_means_in_python() {
        // Missing keys keep the default state's; a non-list doesn't replace a
        // list; unknown keys are ignored; a missing route falls back to scan_film.
        let text = r#"{"simulation": {"film_stock": "kodak_portra_400", "scan_film": true},
            "grain": {"rms_granularity": null, "blur": 0.5, "made_up": 1},
            "halation": "not a section", "load_raw": {"white_balance": "tungsten"}}"#;
        let preset = import(text, &data_dir()).unwrap();
        assert_eq!((preset.film.as_str(), preset.paper.as_str()), ("kodak_portra_400", "kodak_supra_endura"));
        let p = &preset.params;
        assert!(p.io.scan_film);
        assert_eq!(p.film_render.grain.rms_granularity, Some([5.0; 3]));
        assert_eq!(p.film_render.grain.blur, 0.5);
        assert_eq!(p.film_render.halation.halation_amount, 1.0);
        assert_eq!(p.io.raw_white_balance.mode, spektrafilm_io::RawWhiteBalanceMode::Tungsten);
        // The default state is Gold 200's: its DIR gammas, not Portra's.
        assert_eq!(p.film_render.dir_couplers.gamma_samelayer_rgb, Some([0.5159, 0.5934, 0.2829]));
    }

    #[test]
    fn python_infinity_is_a_linear_langmuir_knee_and_nothing_else() {
        let text = r#"{"couplers": {"langmuir_donor_k_rgb": [Infinity, 0.5, Infinity]},
            "display": {"output_interpolation": "Infinity"}}"#;
        let preset = import(text, &data_dir()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(preset.params.film_render.dir_couplers.langmuir_donor_k_rgb, [f64::INFINITY, 0.5, f64::INFINITY]);
        for (text, field) in [
            (r#"{"couplers": {"langmuir_donor_k_rgb": [-Infinity, 1, 1]}}"#, "couplers.langmuir_donor_k_rgb"),
            (r#"{"couplers": {"amount": NaN}}"#, "couplers.amount"),
            (r#"{"scanner": {"unsharp_mask": [Infinity, 0.7]}}"#, "scanner.unsharp_mask"),
        ] {
            let error = import(text, &data_dir()).unwrap_err();
            assert!(error.contains(field), "{error}");
        }
    }

    #[test]
    fn refusals_name_the_reason() {
        for (text, reason) in [
            ("[1, 2]", "no settings"),
            ("{", "valid JSON"),
            (r#"{"simulation": {"film_stock": "kodak_nonexistent"}}"#, "doesn't have the film"),
            (r#"{"simulation": {"film_stock": "../profiles/kodak_gold_200"}}"#, "doesn't have the film"),
            (r#"{"simulation": {"film_stock": "kodak_supra_endura"}}"#, "isn't a film"),
            (r#"{"simulation": {"print_paper": "kodak_portra_400"}}"#, "isn't a print paper"),
            (r#"{"simulation": {"film_stock": 7}}"#, "isn't a stock name"),
            (r#"{"simulation": {"route": "input > film > print"}}"#, "another route"),
            (r#"{"simulation": {"output_color_space": "Display P3"}}"#, "Display P3"),
            (r#"{"special": {"film_channel_swap": [0, 2, 1]}}"#, "swaps colour channels"),
            (r#"{"output_gamut_compress": {"algorithm": "jzazbz"}}"#, "jzazbz"),
            (r#"{"output_gamut_compress": {"knee": [1.5, 1, 1]}}"#, "invalid knee"),
            (r#"{"input_gamut_compress": {"algorithm": "cam16ucs"}}"#, "cam16ucs"),
            (r#"{"input_gamut_compress": {"active": "yes"}}"#, "true or false"),
            (r#"{"input_gamut_compress": {"boundary": "circle"}}"#, "circle"),
            (r#"{"grain": {"blur": "soft"}}"#, "grain.blur"),
            (r#"{"grain": {"rms_granularity": [1, 2]}}"#, "grain.rms_granularity"),
            (r#"{"halation": {"halation_n_bounces": 2.5}}"#, "halation.halation_n_bounces"),
            (r#"{"simulation": {"print_illuminant": "F11"}}"#, "F11"),
            (r#"{"camera": {"color_filter": "wratten_25"}}"#, "wratten_25"),
            (r#"{"camera": {"auto_exposure_method": "spot"}}"#, "spot"),
            (r#"{"camera_diffusion": {"filter_family": "vaseline"}}"#, "vaseline"),
            (r#"{"input_image": {"rgb_to_raw_method": "magic"}}"#, "magic"),
            (r#"{"load_raw": {"white_balance": "flash"}}"#, "load_raw.white_balance"),
            (r#"{"load_raw": {"white_balance": "custom", "temperature": 100}}"#, "temperature"),
        ] {
            let error = import(text, &data_dir()).map(|_| ()).unwrap_err();
            assert!(error.contains(reason), "{text}: {error}");
        }
    }

    #[test]
    fn legacy_and_dataclass_forms_load_as_in_python() {
        let text = r#"{"input_image": {"spectral_upsampling_method": "mallett2019"},
            "simulation": {"selection": {"film_stock": "kodak_ektar_100"}, "enlarger": {"print_exposure": 1.5},
                           "route": "input > film > scan"},
            "gui_only": {"load_raw": {"white_balance": "as_shot"}}}"#;
        let preset = import(text, &data_dir()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(preset.film, "kodak_ektar_100");
        assert_eq!(preset.params.settings.rgb_to_raw_method, "mallett2019");
        assert_eq!(preset.params.enlarger.print_exposure, 1.5);
        assert!(preset.params.io.scan_film);
        assert_eq!(preset.params.io.raw_white_balance.mode, spektrafilm_io::RawWhiteBalanceMode::AsShot);
    }

    #[test]
    fn applying_keeps_the_photograph_s_settings() {
        let preset = import(DEFAULT_GUI_STATE, &data_dir()).unwrap();
        let mut current = RuntimeParams::default();
        current.io.photo_long_side_px = Some(3000);
        current.io.rotate_cw = 1;
        current.settings.use_cat16 = false;
        current.post.contrast = 0.3;
        current.camera.exposure_compensation_ev = 2.0;
        let applied = apply(&preset.params, &current);
        assert_eq!(applied.io.photo_long_side_px, Some(3000));
        assert_eq!(applied.io.rotate_cw, 1);
        assert!(!applied.settings.use_cat16);
        assert_eq!(applied.post.contrast, 0.3);
        assert_eq!(applied.camera.exposure_compensation_ev, 0.0);
    }

    #[test]
    fn export_resolves_stock_values_and_reads_back() {
        let data = data_dir();
        let mut params = RuntimeParams::default();
        params.camera.exposure_compensation_ev = 0.7;
        params.io.input_gamut_compress.algorithm = "off".into();
        params.io.scan_film = true;
        params.film_render.dir_couplers.langmuir_receiver_k_rgb = [f64::INFINITY, 2.0, f64::INFINITY];
        params.film_render.halation.halation_strength = Some([0.1, 0.02, 0.0]);
        let text = export("kodak_portra_400", "kodak_portra_endura", &params, &data).unwrap();
        assert!(text.contains("Infinity,"), "{text}");
        assert!(text.contains("0.7,") || text.contains("0.7\n"), "{text}");
        let preset = import(&text, &data).unwrap();
        assert_eq!((preset.film.as_str(), preset.paper.as_str()), ("kodak_portra_400", "kodak_portra_endura"));
        let p = &preset.params;
        assert_eq!(p.camera.exposure_compensation_ev, 0.7);
        assert_eq!(p.io.input_gamut_compress.algorithm, "off");
        assert!(p.io.scan_film);
        assert_eq!(p.film_render.dir_couplers.langmuir_receiver_k_rgb, [f64::INFINITY, 2.0, f64::INFINITY]);
        assert_eq!(p.film_render.halation.halation_strength, Some([0.1, 0.02, 0.0]));
        // Unset values come back as Portra 400's presets.
        assert_eq!(p.film_render.dir_couplers.gamma_samelayer_rgb, Some([0.5159, 0.5934, 0.2829]));
        assert_eq!(p.film_render.halation.halation_first_sigma_um, Some([65.0; 3]));
        assert!(p.film_render.grain.rms_granularity.is_some());
    }

    #[test]
    fn imported_development_times_are_the_listed_ones() {
        let text = r#"{"simulation": {"film_stock": "kodak_doublex", "print_paper": "kodak_2302"},
            "film_chemistry": {"development_time": 8.0}, "chemistry": {"development_time": null}}"#;
        let p = import(text, &data_dir()).unwrap().params;
        // Double-X lists 4, 5, 6.5, 9 and 12 minutes; 8 develops as 9.
        assert_eq!(p.film_render.development_time, Some(9.0));
        assert_eq!(p.print_render.development_time, None);
        let colour = r#"{"film_chemistry": {"development_time": 1.0}}"#;
        assert_eq!(import(colour, &data_dir()).unwrap().params.film_render.development_time, None);
    }

    #[test]
    fn export_writes_a_black_and_white_family_s_default_time() {
        let data = data_dir();
        let mut params = RuntimeParams::default();
        params.print_render.development_time = Some(7.0);
        let text = export("kodak_doublex", "kodak_2302", &params, &data).unwrap();
        let state: Value = serde_json::from_str(&text).unwrap();
        // Double-X develops for 4, 5, 6.5, 9 or 12 minutes; 2302 keeps its set time.
        assert_eq!(state["film_chemistry"]["development_time"], 6.5);
        assert_eq!(state["chemistry"]["development_time"], 7.0);
        let text = export("kodak_gold_200", "kodak_portra_endura", &params, &data).unwrap();
        let state: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(state["film_chemistry"]["development_time"], Value::Null);
    }
}
