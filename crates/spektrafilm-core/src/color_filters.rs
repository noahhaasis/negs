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
/// and a label for the UI.
pub struct ColorFilterSpec {
    pub key: &'static str,
    pub brand: &'static str,
    pub name: &'static str,
    pub label: &'static str,
}

pub const COLOR_FILTER_LIBRARY: &[ColorFilterSpec] = &[
    ColorFilterSpec { key: "hoya_x0", brand: "hoya", name: "x0", label: "Hoya X0 (yellow-green)" },
    ColorFilterSpec { key: "hoya_x1", brand: "hoya", name: "x1", label: "Hoya X1 (green)" },
    ColorFilterSpec { key: "hoya_y2", brand: "hoya", name: "y2", label: "Hoya Y2 (yellow)" },
    ColorFilterSpec { key: "hoya_ya3", brand: "hoya", name: "ya3", label: "Hoya YA3 (deep yellow)" },
    ColorFilterSpec { key: "hoya_r1", brand: "hoya", name: "r1", label: "Hoya R1 (red)" },
];

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
