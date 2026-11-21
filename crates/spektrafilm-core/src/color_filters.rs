//! Camera taking filters: measured colour-glass transmittances in front of
//! the lens, upstream experimental's `COLOR_FILTER_LIBRARY`
//! (`model/color_filters.py`, 28bf883). The film's spectral sensitivity is
//! multiplied by the transmittance with no renormalisation: the attenuation
//! and the colour cast are the effect, and auto exposure makes up the light.
//!
//! The curves are upstream's `data/filters/colored/<brand>/<name>.csv`,
//! copied unchanged (see `data/filters/colored/README.md`).
use std::path::Path;

use spektrafilm_math::interp::akima_1d;
use spektrafilm_math::spectral::{N_WAVELENGTHS, WAVELENGTH_MIN, WAVELENGTH_STEP};

/// `camera.color_filter` value for no taking filter.
pub const NO_COLOR_FILTER: &str = "none";

/// A filter in the library: the params key, the data folder and file stem,
/// upstream's label, and what the apps call it: its colour and the glass.
pub struct ColorFilterSpec {
    pub key: &'static str,
    pub brand: &'static str,
    pub name: &'static str,
    pub label: &'static str,
    /// The colour photographers name the filter by. YA3 is "Orange": Hoya
    /// sells it as "YA3 Pro Orange" (a Wratten 21), and it lets half the
    /// light through at about 563 nm, where orange filters sit; upstream's
    /// label says deep yellow.
    pub colour: &'static str,
    pub glass: &'static str,
}

pub const COLOR_FILTER_LIBRARY: &[ColorFilterSpec] = &[
    ColorFilterSpec { key: "hoya_x0", brand: "hoya", name: "x0", label: "Hoya X0 (yellow-green)", colour: "Yellow-green", glass: "Hoya X0" },
    ColorFilterSpec { key: "hoya_x1", brand: "hoya", name: "x1", label: "Hoya X1 (green)", colour: "Green", glass: "Hoya X1" },
    ColorFilterSpec { key: "hoya_y2", brand: "hoya", name: "y2", label: "Hoya Y2 (yellow)", colour: "Yellow", glass: "Hoya Y2" },
    ColorFilterSpec { key: "hoya_ya3", brand: "hoya", name: "ya3", label: "Hoya YA3 (deep yellow)", colour: "Orange", glass: "Hoya YA3" },
    ColorFilterSpec { key: "hoya_r1", brand: "hoya", name: "r1", label: "Hoya R1 (red)", colour: "Red", glass: "Hoya R1" },
];

/// The order the apps show the filters in: by the colour they pass, from
/// green to red.
pub const APP_ORDER: [&str; 5] = ["hoya_x1", "hoya_x0", "hoya_y2", "hoya_ya3", "hoya_r1"];

/// The filter's transmittance on the 380–780 nm / 5 nm grid, or `None` for
/// [`NO_COLOR_FILTER`]. Upstream `load_filter`: the CSV's (wavelength,
/// transmittance) rows, deduplicated on wavelength keeping the first row and
/// sorted (`np.unique`), through an Akima interpolator.
pub fn transmittance(data_dir: &Path, key: &str) -> Result<Option<Vec<f64>>, String> {
    if key == NO_COLOR_FILTER {
        return Ok(None);
    }
    let spec = COLOR_FILTER_LIBRARY
        .iter()
        .find(|spec| spec.key == key)
        .ok_or_else(|| format!("unknown camera color filter: {key:?}"))?;
    let path = data_dir
        .join("filters/colored")
        .join(spec.brand)
        .join(format!("{}.csv", spec.name));
    let text = crate::data_fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut rows = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let parse = |field: Option<&str>| -> Result<f64, String> {
            field
                .and_then(|f| f.trim().parse().ok())
                .ok_or_else(|| format!("{}: invalid row {line:?}", path.display()))
        };
        let mut fields = line.split(',');
        rows.push((parse(fields.next())?, parse(fields.next())?));
    }
    // A stable sort keeps the first of equal wavelengths first, as
    // `np.unique(..., return_index=True)`.
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.dedup_by(|later, first| later.0 == first.0);
    let (x, y): (Vec<f64>, Vec<f64>) = rows.into_iter().unzip();
    if x.len() < 2 {
        return Err(format!("{}: needs two rows or more", path.display()));
    }
    let wavelengths: Vec<f64> = (0..N_WAVELENGTHS)
        .map(|i| WAVELENGTH_MIN as f64 + WAVELENGTH_STEP as f64 * i as f64)
        .collect();
    let values = akima_1d(&x, &y, &wavelengths);
    if values.iter().any(|v| !v.is_finite()) {
        return Err(format!("{} does not cover 380–780 nm", path.display()));
    }
    Ok(Some(values))
}

/// How much light the filter takes from the film in daylight, in stops: the
/// filter factor makers quote, for this film. The film's own sensitivity
/// under its reference illuminant (D55 for camera films) weighs the glass's
/// transmittance, as an exposure meter behind the filter would.
pub fn factor_stops(film: &crate::profile::Profile, transmittance: &[f64]) -> Result<f64, String> {
    let light = crate::illuminants::standard_illuminant_f64(&film.info.reference_illuminant)?;
    let (mut open, mut filtered) = (0.0, 0.0);
    for ((row, e), t) in film.log_sensitivity_f64().iter().zip(light.iter()).zip(transmittance) {
        // Unmeasured wavelengths are null (NaN): no sensitivity, as the pipeline reads them.
        let s = 10f64.powf(row[0]);
        let s = if s.is_finite() { s } else { 0.0 };
        open += s * e;
        filtered += s * e * t;
    }
    if !(open > 0.0 && filtered > 0.0) {
        return Err(format!("{} has no sensitivity the filter passes", film.info.name.as_deref().unwrap_or("the film")));
    }
    Ok((open / filtered).log2())
}

/// The glass's colour laid on white paper in daylight (D65), as sRGB bytes,
/// for the apps' swatches: the transmitted light relative to the paper's
/// white, so a deep red stays darker than a yellow. Colours outside sRGB
/// keep their hue and lose some saturation.
pub fn glass_srgb(transmittance: &[f64]) -> [u8; 3] {
    use spektrafilm_math::spectral::{CMF_X_F64, CMF_Y_F64, CMF_Z_F64, ILLUMINANT_D65_F64};
    let mut xyz = [0.0f64; 3];
    let mut white = 0.0;
    for (i, t) in transmittance.iter().enumerate() {
        let e = ILLUMINANT_D65_F64[i] * t;
        xyz[0] += e * CMF_X_F64[i];
        xyz[1] += e * CMF_Y_F64[i];
        xyz[2] += e * CMF_Z_F64[i];
        white += ILLUMINANT_D65_F64[i] * CMF_Y_F64[i];
    }
    let xyz = xyz.map(|v| v / white);
    // XYZ to linear sRGB (IEC 61966-2-1).
    let m = [[3.2404542, -1.5371385, -0.4985314], [-0.9692660, 1.8760108, 0.0415560], [0.0556434, -0.2040259, 1.0572252]];
    let mut rgb = m.map(|r| r[0] * xyz[0] + r[1] * xyz[1] + r[2] * xyz[2]);
    // Into gamut: lift negatives towards the colour's own grey, then bring
    // a channel above 1 down to it.
    let grey = xyz[1];
    let low = rgb.iter().copied().fold(f64::INFINITY, f64::min);
    if low < 0.0 {
        let k = grey / (grey - low);
        rgb = rgb.map(|c| grey + (c - grey) * k);
    }
    let high = rgb.iter().copied().fold(1.0, f64::max);
    rgb.map(|c| {
        let c = (c / high).clamp(0.0, 1.0);
        let encoded = if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (encoded * 255.0).round() as u8
    })
}
