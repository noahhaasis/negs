//! Translate presentation names to the live helper's parameter schema. No film math.
use crate::model::State;
use serde_json::{Value, json};

pub fn get<'a>(value: &'a Value, path: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    path.split('.').fold(value, |v, key| match v {
        Value::Array(a) => key
            .parse::<usize>()
            .ok()
            .and_then(|i| a.get(i))
            .unwrap_or(&NULL),
        _ => v.get(key).unwrap_or(&NULL),
    })
}
pub fn set(value: &mut Value, path: &str, new: Value) {
    fn descend(value: &mut Value, keys: &[&str], new: Value) {
        let Some((key, rest)) = keys.split_first() else {
            *value = new;
            return;
        };
        match value {
            Value::Object(map) => {
                if let Some(child) = map.get_mut(*key) {
                    descend(child, rest, new);
                }
            }
            Value::Array(items) => {
                if let Some(child) = key.parse::<usize>().ok().and_then(|i| items.get_mut(i)) {
                    descend(child, rest, new);
                }
            }
            _ => {}
        }
    }
    descend(value, &path.split('.').collect::<Vec<_>>(), new);
}
pub fn number(value: f64) -> Value {
    if value.fract() == 0. && value.abs() < 9e15 {
        json!(value as i64)
    } else {
        json!(value)
    }
}

pub fn path(key: &str) -> Option<String> {
    let (group, name) = key.split_once('.')?;
    let direct = match (group, name) {
        ("Camera", "Exposure") => "camera.exposure_compensation_ev",
        ("Camera", "Film format") => "camera.film_format_mm",
        ("Camera", "Lens blur") => "camera.lens_blur_um",
        ("RAW", "Temperature") => "io.raw_white_balance.temperature",
        ("RAW", "Tint") => "io.raw_white_balance.tint",
        ("Grain", "Red granularity") => "film_render.grain.rms_granularity.0",
        ("Grain", "Green granularity") => "film_render.grain.rms_granularity.1",
        ("Grain", "Blue granularity") => "film_render.grain.rms_granularity.2",
        ("Grain", "Red uniformity") => "film_render.grain.uniformity.0",
        ("Grain", "Green uniformity") => "film_render.grain.uniformity.1",
        ("Grain", "Blue uniformity") => "film_render.grain.uniformity.2",
        ("Grain", "Coarse grain size") => "film_render.grain.particle_scale_sublayers.0",
        ("Grain", "Medium grain size") => "film_render.grain.particle_scale_sublayers.1",
        ("Grain", "Fine grain size") => "film_render.grain.particle_scale_sublayers.2",
        ("Grain", "Dye clouds") => "film_render.grain.blur_dye_clouds_um",
        ("Grain", "Post blur") => "film_render.grain.blur",
        ("Grain", "Sharpen amount") => "film_render.grain.mult_usm_amount",
        ("Grain", "Sharpen radius") => "film_render.grain.mult_usm_sigma",
        ("Grain", "Clump blur") => "film_render.grain.micro_structure.0",
        ("Grain", "Clump size") => "film_render.grain.micro_structure.1",
        ("Halation", "Amount") => "film_render.halation.halation_amount",
        ("Halation", "Spatial scale") => "film_render.halation.halation_spatial_scale",
        ("Halation", "Scatter amount") => "film_render.halation.scatter_amount",
        ("Halation", "Scatter scale") => "film_render.halation.scatter_spatial_scale",
        ("Halation", "Bounces") => "film_render.halation.halation_n_bounces",
        ("Halation", "Bounce decay") => "film_render.halation.halation_bounce_decay",
        ("DIR Couplers", "Amount") => "film_render.dir_couplers.amount",
        ("DIR Couplers", "Diffusion") => "film_render.dir_couplers.diffusion_size_um",
        ("DIR Couplers", "Tail") => "film_render.dir_couplers.diffusion_tail_um",
        ("DIR Couplers", "Tail weight") => "film_render.dir_couplers.diffusion_tail_weight",
        ("Enlarger", "Print exposure") => "enlarger.print_exposure",
        ("Enlarger", "Magenta shift") => "enlarger.m_filter_shift",
        ("Enlarger", "Yellow shift") => "enlarger.y_filter_shift",
        ("Enlarger", "Preflash exposure") => "enlarger.preflash_exposure",
        ("Enlarger", "Preflash magenta") => "enlarger.preflash_m_filter_shift",
        ("Enlarger", "Preflash yellow") => "enlarger.preflash_y_filter_shift",
        ("Glare", "Amount") => "print_render.glare.percent",
        ("Glare", "Roughness") => "print_render.glare.roughness",
        ("Glare", "Blur") => "print_render.glare.blur",
        ("Scanner", "Lens blur") => "scanner.lens_blur",
        ("Scanner", "Unsharp radius") => "scanner.unsharp_mask.0",
        ("Scanner", "Unsharp amount") => "scanner.unsharp_mask.1",
        ("Scanner", "White level") => "scanner.white_level",
        ("Scanner", "Black level") => "scanner.black_level",
        _ => "",
    };
    if !direct.is_empty() {
        return Some(direct.into());
    }
    if matches!(group, "Film Curves" | "Print Curves") {
        let field = match name {
            "Gamma" => "gamma_factor",
            "Fast gamma" => "gamma_factor_fast",
            "Slow gamma" => "gamma_factor_slow",
            "Red gamma" => "gamma_factor_red",
            "Green gamma" => "gamma_factor_green",
            "Blue gamma" => "gamma_factor_blue",
            "Developer exhaustion" => "developer_exhaustion",
            _ => return None,
        };
        return Some(format!(
            "{}_render.density_curves_morph.{field}",
            if group == "Film Curves" {
                "film"
            } else {
                "print"
            }
        ));
    }
    if matches!(group, "Film Base" | "Print Base") {
        let field = match name {
            "Density" => "scale",
            "Tilt" if group == "Film Base" => "tilt",
            "Cyan" => "cyan",
            "Magenta" => "magenta",
            "Yellow" => "yellow",
            _ => return None,
        };
        return Some(format!(
            "{}_render.base.{field}",
            if group == "Film Base" {
                "film"
            } else {
                "print"
            }
        ));
    }
    if group == "Lens Diffusion" {
        return Some(format!(
            "camera.diffusion_filter.{}",
            name.to_lowercase().replace(' ', "_")
        ));
    }
    if group == "Post" {
        return Some(format!("post.{}", name.to_lowercase()));
    }
    None
}

/// Grain arrays that stay null until edited, so the film's preset from the
/// catalog applies (`Stock.grainFields` in `Parameters.swift`).
pub const GRAIN_STOCK_FIELDS: [&str; 4] = [
    "rms_granularity",
    "density_min",
    "uniformity",
    "particle_scale_sublayers",
];

/// A control on one lane of a grain preset array: its field and lane index.
pub fn grain_lane(key: &str) -> Option<(&'static str, usize)> {
    let path = path(key)?;
    let (field, index) = path.strip_prefix("film_render.grain.")?.split_once('.')?;
    let field = GRAIN_STOCK_FIELDS.into_iter().find(|f| *f == field)?;
    Some((field, index.parse().ok()?))
}
/// B&W films have one emulsion: per-channel lanes past the first are hidden.
pub fn color_only(key: &str) -> bool {
    grain_lane(key).is_some_and(|(field, index)| field != "particle_scale_sublayers" && index > 0)
}
/// The first lane's title on a B&W film, without its colour.
pub fn bw_title(key: &str) -> Option<&'static str> {
    match grain_lane(key)? {
        ("rms_granularity", 0) => Some("Granularity"),
        ("uniformity", 0) => Some("Uniformity"),
        _ => None,
    }
}
/// The selected film's preset for a lane, from the catalog.
pub fn grain_preset(state: &State, field: &str, index: usize) -> Option<f64> {
    crate::catalog::selected(state, false).grain[field][index].as_f64()
}
fn same(a: f64, b: f64) -> bool {
    // Rust defaults arrive as widened f32 values; slider edits are decimal.
    (a - b).abs() <= 1e-6 * b.abs().max(1.)
}
/// Set one preset lane. Back at the preset in every lane, the field is unset
/// again, so a later film change applies that film's preset.
pub fn set_grain_lane(state: &mut State, key: &str, value: f64) {
    let Some((field, _)) = grain_lane(key) else {
        return;
    };
    state.values.insert(key.into(), value);
    let lanes: Vec<_> = state
        .values
        .keys()
        .filter(|k| grain_lane(k).is_some_and(|(f, _)| f == field))
        .cloned()
        .collect();
    if lanes.iter().all(|k| {
        let (_, index) = grain_lane(k).unwrap();
        grain_preset(state, field, index).is_some_and(|preset| same(state.values[k], preset))
    }) {
        for k in lanes {
            state.values.remove(&k);
        }
    }
}
/// A new film brings its own grain preset, as upstream applies it.
pub fn clear_grain_lanes(state: &mut State) {
    state.values.retain(|k, _| grain_lane(k).is_none());
}

pub const TOGGLES: &[(&str, &str)] = &[
    ("Auto exposure", "camera.auto_exposure"),
    ("Scan film directly", "io.scan_film"),
    (
        "Film Curves enabled",
        "film_render.density_curves_morph.active",
    ),
    ("Grain enabled", "film_render.grain.active"),
    ("Halation enabled", "film_render.halation.active"),
    ("DIR Couplers enabled", "film_render.dir_couplers.active"),
    ("Film Base enabled", "film_render.base.active"),
    ("Lens Diffusion enabled", "camera.diffusion_filter.active"),
    (
        "Print Curves enabled",
        "print_render.density_curves_morph.active",
    ),
    ("Print Base enabled", "print_render.base.active"),
    ("Glare enabled", "print_render.glare.active"),
    (
        "Exposure compensation",
        "enlarger.print_exposure_compensation",
    ),
    ("White correction", "scanner.white_correction"),
    ("Black correction", "scanner.black_correction"),
    ("CAT16 adaptation", "settings.use_cat16"),
    ("Encode output as sRGB", "io.output_cctf_encoding"),
    ("Use less memory for export", "settings.serial_grain"),
];
pub const CHOICES: &[(&str, &str, &[&str])] = &[
    (
        "Metering",
        "camera.auto_exposure_method",
        &[
            "average",
            "median",
            "center_weighted",
            "partial",
            "matrix",
            "multi_zone",
            "highlight_weighted",
        ],
    ),
    (
        "White balance",
        "io.raw_white_balance.mode",
        &["as_shot", "daylight", "tungsten", "custom"],
    ),
    (
        "Filter",
        "camera.diffusion_filter.filter_family",
        &["black_pro_mist", "glimmerglass", "pro_mist", "cinebloom"],
    ),
    (
        "Output gamut",
        "io.output_gamut_compress.algorithm",
        &["oklch", "off", "oklrab", "cam16ucs", "aces_rgc"],
    ),
    (
        "Input gamut",
        "io.input_gamut_compress.algorithm",
        &["off", "xy"],
    ),
    (
        "Spectral upsampling",
        "settings.rgb_to_raw_method",
        &["hanatos2025", "arctic2026beta04", "mallett2019"],
    ),
];

/// Check the controls against the helper's `catalog` reply: its defaults and,
/// for the grain preset lanes, every film's resolved preset.
pub fn validate(catalog: &Value) -> Result<(), String> {
    let defaults = &catalog["defaults"];
    for group in crate::fixtures::GROUPS {
        for knob in group.knobs {
            let key = format!("{}.{}", group.name, knob.0);
            let field = path(&key).ok_or_else(|| format!("No engine binding for {key}"))?;
            if let Some((name, index)) = grain_lane(&key) {
                let array = format!("film_render.grain.{name}");
                if !get(defaults, &array).is_null() {
                    return Err(format!("The engine does not leave {array} to the film."));
                }
                let films = catalog["profiles"].as_array().into_iter().flatten();
                for film in films.filter(|p| p["paper"] == false) {
                    if !film["grain"][name][index].is_number() {
                        return Err(format!(
                            "The engine catalog has no {name} preset for {}.",
                            film["id"]
                        ));
                    }
                }
                continue;
            }
            if !get(defaults, &field).is_number() {
                return Err(format!("The engine has no numeric parameter {field}."));
            }
        }
    }
    for &(title, field) in TOGGLES {
        if !get(defaults, field).is_boolean() {
            return Err(format!("The engine has no switch for {title}."));
        }
    }
    for &(title, field, options) in CHOICES {
        if !options
            .iter()
            .any(|&v| get(defaults, field).as_str() == Some(v))
        {
            return Err(format!("The engine uses an unsupported {title} option."));
        }
    }
    Ok(())
}

/// Fill inspector values from actual defaults. Preserve explicit UI edits during
/// helper startup; an old preview fixture's unchanged sample values are migrated.
pub fn install_defaults(state: &mut State, defaults: &Value, reset: bool) {
    let fixture = crate::fixtures::state();
    if reset || !state.choices.contains_key("Limit preview to 12 MP") {
        state
            .choices
            .insert("Limit preview to 12 MP".into(), "false".into());
    }
    for group in crate::fixtures::GROUPS {
        for knob in group.knobs {
            apply_default(
                state,
                &fixture,
                defaults,
                &format!("{}.{}", group.name, knob.0),
                reset,
            );
        }
    }
    for knob in crate::fixtures::POST {
        apply_default(
            state,
            &fixture,
            defaults,
            &format!("Post.{}", knob.0),
            reset,
        );
    }
    for key in ["RAW.Temperature", "RAW.Tint"] {
        apply_default(state, &fixture, defaults, key, reset);
    }
    for &(key, path) in TOGGLES {
        if (reset || !state.choices.contains_key(key))
            && let Some(value) = get(defaults, path).as_bool()
        {
            state.choices.insert(key.into(), value.to_string());
        }
    }
    for &(key, path, options) in CHOICES {
        if (reset || !state.choices.contains_key(key))
            && let Some(index) = options
                .iter()
                .position(|&s| get(defaults, path).as_str() == Some(s))
        {
            state.choices.insert(key.into(), index.to_string());
        }
    }
}
fn apply_default(state: &mut State, fixture: &State, defaults: &Value, key: &str, reset: bool) {
    if (reset
        || !state.values.contains_key(key)
        || (state.params.is_null() && state.values.get(key) == fixture.values.get(key)))
        && let Some(path) = path(key)
        && let Some(value) = get(defaults, &path).as_f64()
    {
        state.values.insert(key.into(), value);
    }
}

/// RuntimeParams stays complete: fields without controls keep the engine's values.
pub fn runtime(state: &State, defaults: &Value) -> Value {
    let mut params = merge(defaults, &state.params);
    for (key, value) in &state.values {
        if value.is_finite()
            && grain_lane(key).is_none()
            && let Some(path) = path(key)
        {
            set(&mut params, &path, number(*value));
        }
    }
    // An edited preset field materializes whole from the film's preset.
    let preset = &crate::catalog::selected(state, false).grain;
    for field in GRAIN_STOCK_FIELDS {
        let mut lanes: Option<Vec<Value>> = None;
        for (key, value) in &state.values {
            if let Some((f, index)) = grain_lane(key)
                && f == field
                && value.is_finite()
                && let Some(lane) = lanes
                    .get_or_insert_with(|| preset[field].as_array().cloned().unwrap_or_default())
                    .get_mut(index)
            {
                *lane = number(*value);
            }
        }
        set(
            &mut params,
            &format!("film_render.grain.{field}"),
            lanes.map_or(Value::Null, Value::Array),
        );
    }
    for &(key, path) in TOGGLES {
        if let Some(value) = state.choices.get(key) {
            set(&mut params, path, json!(value == "true"));
        }
    }
    for &(key, path, options) in CHOICES {
        if let Some(value) = state
            .choices
            .get(key)
            .and_then(|s| s.parse::<usize>().ok())
            .and_then(|i| options.get(i))
        {
            set(&mut params, path, json!(value));
        }
    }
    for (key, path, paper) in [
        ("Development", "film_render.development_time", false),
        ("Print development", "print_render.development_time", true),
    ] {
        let index = state
            .choices
            .get(key)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        let stock = crate::catalog::selected(state, paper);
        set(
            &mut params,
            path,
            index
                .checked_sub(1)
                .and_then(|i| stock.times.get(i))
                .map_or(Value::Null, |n| json!(n)),
        );
    }
    set(&mut params, "io.upscale_factor", json!(1));
    set(&mut params, "io.rotate_cw", json!(0));
    let target = (state.choices.get("Develop at").map(String::as_str) == Some("1")).then(|| {
        state
            .values
            .get("Image Size.Longest side")
            .copied()
            .unwrap_or(2048.) as u32
    });
    set(&mut params, "io.photo_long_side_px", json!(target));
    params
}

pub fn merge(defaults: &Value, saved: &Value) -> Value {
    match (defaults, saved) {
        (Value::Object(d), Value::Object(s)) => {
            let mut out = d.clone();
            for (key, value) in s {
                out.insert(
                    key.clone(),
                    d.get(key)
                        .map_or_else(|| value.clone(), |old| merge(old, value)),
                );
            }
            Value::Object(out)
        }
        (Value::Array(d), Value::Array(s)) if d.len() == s.len() => {
            Value::Array(d.iter().zip(s).map(|(a, b)| merge(a, b)).collect())
        }
        (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_))
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Null, _) => saved.clone(),
        _ => defaults.clone(),
    }
}

pub fn crop(state: &State) -> Value {
    let [x, y, width, height] = state.crop.unwrap_or([0., 0., 1., 1.]);
    json!({"x":x,"y":y,"width":width,"height":height,"angle":state.straighten})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn film(state: &mut State, id: &str) {
        state.choices.insert("Film ID".into(), id.into());
    }
    fn grain(params: &Value, field: &str) -> Value {
        // Compare lanes as numbers: presets arrive as floats, edits as decimals.
        match &params["film_render"]["grain"][field] {
            Value::Array(lanes) => lanes.iter().map(|v| json!(v.as_f64().unwrap())).collect(),
            other => other.clone(),
        }
    }
    fn defaults() -> Value {
        json!({"film_render":{"grain":{"rms_granularity":null,"density_min":null,"uniformity":null,
            "particle_scale_sublayers":null,"blur":0.89},"base":{"active":true,"scale":1.0,"tilt":0.0}},
            "print_render":{"base":{"active":true,"scale":1.0}}})
    }

    #[test]
    fn base_controls_bind_the_engine_paths() {
        for (key, field) in [
            ("Film Base.Density", "film_render.base.scale"),
            ("Film Base.Tilt", "film_render.base.tilt"),
            ("Film Base.Cyan", "film_render.base.cyan"),
            ("Film Base.Magenta", "film_render.base.magenta"),
            ("Film Base.Yellow", "film_render.base.yellow"),
            ("Print Base.Density", "print_render.base.scale"),
            ("Print Base.Cyan", "print_render.base.cyan"),
            ("Print Base.Magenta", "print_render.base.magenta"),
            ("Print Base.Yellow", "print_render.base.yellow"),
        ] {
            assert_eq!(path(key).as_deref(), Some(field));
        }
        assert_eq!(path("Print Base.Tilt"), None);
        let mut state = crate::fixtures::state();
        state.values.insert("Film Base.Tilt".into(), -0.25);
        state
            .choices
            .insert("Print Base enabled".into(), "false".into());
        let params = runtime(&state, &defaults());
        assert_eq!(params["film_render"]["base"]["tilt"], -0.25);
        assert_eq!(params["print_render"]["base"]["active"], false);
    }

    #[test]
    fn grain_lanes_follow_the_stock_preset() {
        assert_eq!(
            grain_lane("Grain.Blue granularity"),
            Some(("rms_granularity", 2))
        );
        assert_eq!(grain_lane("Grain.Clump blur"), None);
        assert_eq!(grain_lane("Grain.Post blur"), None);
        assert!(color_only("Grain.Green uniformity"));
        assert!(!color_only("Grain.Red uniformity"));
        assert!(!color_only("Grain.Fine grain size"));
        assert_eq!(bw_title("Grain.Red granularity"), Some("Granularity"));
        assert_eq!(bw_title("Grain.Red uniformity"), Some("Uniformity"));
        assert_eq!(bw_title("Grain.Coarse grain size"), None);
        let lanes: Vec<_> = crate::fixtures::GROUPS
            .iter()
            .flat_map(|g| g.knobs.iter().map(move |k| format!("{}.{}", g.name, k.0)))
            .filter_map(|k| grain_lane(&k))
            .collect();
        assert_eq!(lanes.len(), 9);
        // Unedited lanes stay null, so the engine applies the film's preset.
        let mut state = crate::fixtures::state();
        assert!(state.values.keys().all(|k| grain_lane(k).is_none()));
        film(&mut state, "kodak_vision3_500t");
        let params = runtime(&state, &defaults());
        for field in GRAIN_STOCK_FIELDS {
            assert_eq!(grain(&params, field), Value::Null);
        }
        // One edited lane materializes the whole array from the preset.
        set_grain_lane(&mut state, "Grain.Green granularity", 20.);
        let params = runtime(&state, &defaults());
        assert_eq!(grain(&params, "rms_granularity"), json!([9., 20., 30.]));
        assert_eq!(grain(&params, "uniformity"), Value::Null);
        set_grain_lane(&mut state, "Grain.Blue uniformity", 0.5);
        assert_eq!(
            grain(&runtime(&state, &defaults()), "uniformity"),
            json!([0.93, 0.97, 0.5])
        );
        // Back at the preset in every lane, the field is null again.
        set_grain_lane(&mut state, "Grain.Green granularity", 13.);
        assert_eq!(
            grain(&runtime(&state, &defaults()), "rms_granularity"),
            Value::Null
        );
        assert!(!state.values.contains_key("Grain.Green granularity"));
        // Choosing another film clears the edits, so its own preset applies.
        clear_grain_lanes(&mut state);
        film(&mut state, "kodak_trix");
        assert_eq!(
            grain(&runtime(&state, &defaults()), "uniformity"),
            Value::Null
        );
        assert_eq!(grain_preset(&state, "rms_granularity", 0), Some(44.));
        assert_eq!(
            grain_preset(&state, "particle_scale_sublayers", 1),
            Some(0.4)
        );
    }

    #[test]
    fn validation_requires_film_presets_for_lanes_only() {
        let catalog = |grain: Value| {
            let mut defaults: Value =
                serde_json::from_str(include_str!("../catalog.json")).unwrap();
            defaults =
                json!({"defaults": defaults_for_validation(), "profiles": defaults["profiles"]});
            for p in defaults["profiles"].as_array_mut().unwrap() {
                if p["paper"] == false {
                    p["grain"] = grain.clone();
                }
            }
            defaults
        };
        let preset = json!({"rms_granularity":[4,4,4],"density_min":[0.03,0.03,0.03],
            "uniformity":[0.97,0.97,0.97],"particle_scale_sublayers":[1,0.4,0.25]});
        assert_eq!(validate(&catalog(preset.clone())), Ok(()));
        let mut short = preset.clone();
        short["uniformity"] = json!([0.97]);
        assert!(
            validate(&catalog(short))
                .unwrap_err()
                .contains("uniformity")
        );
        let mut numeric = catalog(preset);
        numeric["defaults"]["film_render"]["grain"]["rms_granularity"] = json!([1, 1, 1]);
        assert!(validate(&numeric).is_err());
        // Every other knob still needs a numeric default.
        let mut missing = catalog(json!(null));
        missing["profiles"] = json!([]);
        missing["defaults"]["film_render"]["base"]["tilt"] = Value::Null;
        assert!(
            validate(&missing)
                .unwrap_err()
                .contains("film_render.base.tilt")
        );
    }

    /// Defaults with every bound field, built from the controls themselves.
    fn defaults_for_validation() -> Value {
        let mut defaults = json!({});
        let mut put = |path: &str, value: Value| {
            let mut node = &mut defaults;
            let keys: Vec<_> = path.split('.').collect();
            for (i, key) in keys.iter().enumerate() {
                let last = i + 1 == keys.len();
                if let Ok(index) = key.parse::<usize>() {
                    let array = node.as_array_mut().unwrap();
                    while array.len() <= index {
                        array.push(Value::Null);
                    }
                    node = &mut array[index];
                } else {
                    let next_is_index = keys.get(i + 1).is_some_and(|k| k.parse::<usize>().is_ok());
                    node = node
                        .as_object_mut()
                        .unwrap()
                        .entry(key.to_string())
                        .or_insert(if next_is_index { json!([]) } else { json!({}) });
                }
                if last {
                    *node = value.clone();
                }
            }
        };
        for group in crate::fixtures::GROUPS {
            for knob in group.knobs {
                let key = format!("{}.{}", group.name, knob.0);
                match grain_lane(&key) {
                    Some((field, _)) => put(&format!("film_render.grain.{field}"), Value::Null),
                    None => put(&path(&key).unwrap(), json!(knob.4)),
                }
            }
        }
        for (_, field) in TOGGLES {
            put(field, json!(true));
        }
        for (_, field, options) in CHOICES {
            put(field, json!(options[0]));
        }
        defaults
    }

    #[test]
    fn new_edits_start_on_the_mac_stocks() {
        let state = State::default();
        assert_eq!(crate::catalog::selected(&state, false).id, "kodak_gold_200");
        assert_eq!(
            crate::catalog::selected(&state, true).id,
            "kodak_portra_endura"
        );
        // Old sessions keep the stock their menu index named.
        let mut old = State::default();
        old.choices.insert("Film".into(), "0".into());
        assert_eq!(crate::catalog::selected(&old, false).id, "kodak_portra_400");
    }

    #[test]
    #[ignore = "Requires a built native helper; the release recipe runs this integration gate"]
    fn live_helper_schema() {
        let helper = std::env::var_os("NEGS_SCHEMA_HELPER")
            .expect("Set NEGS_SCHEMA_HELPER to the built spektrafilm-native executable");
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let mut child = Command::new(helper)
            .arg("--data-dir")
            .arg(data)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Start the native helper");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"{\"id\":1,\"command\":\"catalog\"}\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "The helper must exit cleanly on EOF"
        );
        let reply: Value = serde_json::from_slice(&output.stdout).expect("Read the helper catalog");
        assert_eq!(reply["id"], 1);
        assert_eq!(reply["result"]["protocol"], 1);
        validate(&reply["result"])
            .expect("The Linux controls must accept the shipped helper's defaults");
        // Every film's preset lane falls inside its slider's range.
        for film in reply["result"]["profiles"].as_array().unwrap() {
            if film["paper"] == true {
                continue;
            }
            for knob in crate::fixtures::GROUPS.iter().flat_map(|g| g.knobs) {
                if let Some((field, index)) = grain_lane(&format!("Grain.{}", knob.0)) {
                    let preset = film["grain"][field][index].as_f64().unwrap();
                    assert!(
                        (knob.1..=knob.2).contains(&preset),
                        "{} {field}.{index} preset {preset} is outside its slider",
                        film["id"]
                    );
                }
            }
        }
    }
}
