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
    ("Lens Diffusion enabled", "camera.diffusion_filter.active"),
    (
        "Print Curves enabled",
        "print_render.density_curves_morph.active",
    ),
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

pub fn validate(defaults: &Value) -> Result<(), String> {
    for group in crate::fixtures::GROUPS {
        for knob in group.knobs {
            let key = format!("{}.{}", group.name, knob.0);
            let field = path(&key).ok_or_else(|| format!("No engine binding for {key}"))?;
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
            && let Some(path) = path(key)
        {
            set(&mut params, &path, number(*value));
        }
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

/// Values a film's presets fill while the edit leaves them null. Choosing a
/// film resets them, as upstream re-applies its presets on a stock change
/// (`Stock.presetPaths` in `Parameters.swift`).
pub const STOCK_PRESET_PATHS: [&str; 10] = [
    "film_render.grain.rms_granularity",
    "film_render.grain.density_min",
    "film_render.grain.uniformity",
    "film_render.grain.particle_scale_sublayers",
    "film_render.dir_couplers.gamma_samelayer_rgb",
    "film_render.dir_couplers.gamma_interlayer_r_to_gb",
    "film_render.dir_couplers.gamma_interlayer_g_to_rb",
    "film_render.dir_couplers.gamma_interlayer_b_to_rg",
    "film_render.halation.halation_strength",
    "film_render.halation.halation_first_sigma_um",
];

/// Session schema 2: the DIR gammas and halation baseline are null unless
/// set, and a set value beats the film's preset. Schema 1 stored values for
/// them that the engine ignored and nothing could set, so restoring drops
/// them (the last six of `STOCK_PRESET_PATHS`).
pub const SESSION_SCHEMA: u64 = 2;

pub fn migrate_session(schema: u64, state: &mut State) {
    if schema < 2 {
        for path in &STOCK_PRESET_PATHS[4..] {
            set(&mut state.params, path, Value::Null);
        }
    }
}

/// The catalog's `nullable_paths`: where a null is a value, such as an
/// infinite Langmuir knee (`[*]` for every element). `merge` keeps a null
/// there instead of falling back to the default.
static NULLABLE_PATHS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub fn set_nullable_paths(catalog: &Value) {
    let paths = catalog["nullable_paths"]
        .as_array()
        .map(|paths| paths.iter().filter_map(|p| p.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    *NULLABLE_PATHS.lock().unwrap_or_else(|e| e.into_inner()) = paths;
}

pub fn merge(defaults: &Value, saved: &Value) -> Value {
    let nullable = NULLABLE_PATHS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    merge_at(defaults, saved, &nullable, "")
}

fn merge_at(defaults: &Value, saved: &Value, nullable: &[String], path: &str) -> Value {
    if saved.is_null() && nullable.iter().any(|p| p == path) {
        return Value::Null;
    }
    match (defaults, saved) {
        (Value::Object(d), Value::Object(s)) => {
            let mut out = d.clone();
            for (key, value) in s {
                let child = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                out.insert(
                    key.clone(),
                    d.get(key)
                        .map_or_else(|| value.clone(), |old| merge_at(old, value, nullable, &child)),
                );
            }
            Value::Object(out)
        }
        (Value::Array(d), Value::Array(s)) if d.len() == s.len() => {
            let element = format!("{path}[*]");
            Value::Array(d.iter().zip(s).map(|(a, b)| merge_at(a, b, nullable, &element)).collect())
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

    #[test]
    fn merge_keeps_saved_nulls_where_they_are_values() {
        let defaults = json!({"dir": {"langmuir": [1, 1, 1], "gamma": null}, "gamut": {"lightness": [0.95, 1, 1.6], "knee": 1}});
        let saved = json!({"dir": {"langmuir": [null, 0.5, null], "gamma": [0.52, 0.51, 0.26]}, "gamut": {"lightness": null, "knee": null}});
        let nullable = ["dir.langmuir[*]", "dir.gamma", "gamut.lightness"].map(String::from);
        assert_eq!(
            merge_at(&defaults, &saved, &nullable, ""),
            json!({"dir": {"langmuir": [null, 0.5, null], "gamma": [0.52, 0.51, 0.26]}, "gamut": {"lightness": null, "knee": 1}})
        );
        assert_eq!(merge_at(&defaults, &saved, &[], "")["dir"]["langmuir"], json!([1, 0.5, 1]));
    }

    #[test]
    fn schema_1_sessions_drop_dir_and_halation_values_that_never_applied() {
        let params = json!({"film_render": {
            "dir_couplers": {"gamma_samelayer_rgb": [0.341, 0.324, 0.273], "amount": 0.8},
            "halation": {"halation_strength": [0.05, 0.015, 0.0]},
            "grain": {"rms_granularity": [12, 13, 12]}}});
        let mut old = State { params: params.clone(), ..Default::default() };
        migrate_session(1, &mut old);
        let render = &old.params["film_render"];
        assert!(render["dir_couplers"]["gamma_samelayer_rgb"].is_null());
        assert!(render["halation"]["halation_strength"].is_null());
        assert_eq!(render["dir_couplers"]["amount"], json!(0.8));
        assert_eq!(render["grain"]["rms_granularity"], json!([12, 13, 12]));
        let mut current = State { params: params.clone(), ..Default::default() };
        migrate_session(SESSION_SCHEMA, &mut current);
        assert_eq!(current.params, params);
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
        validate(&reply["result"]["defaults"])
            .expect("The Linux controls must accept the shipped helper's defaults");
    }
}
