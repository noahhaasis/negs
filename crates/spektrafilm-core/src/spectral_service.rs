/// Spectral upsampling service: loads the Hanatos2025 spectra LUT and computes
/// the TC LUT for a given film stock's sensitivity.
///
/// Port of Python `compute_hanatos2025_tc_lut` and `_load_hanatos2025_spectra_lut`.
use std::path::Path;

use spektrafilm_math::npy;
use spektrafilm_math::spectral::{self, N_WAVELENGTHS, TcLut};

// Pull in BLAS for the spectra→tc_lut contraction (matches Python's
// opt_einsum + numpy summation pattern bit-for-bit on macOS Accelerate).
#[cfg(not(target_os = "windows"))]
#[allow(unused_imports)]
use blas_src as _;

fn dgemm_no_transpose(a: &[f64], b: &[f64], c: &mut [f64], m: usize, n: usize, k: usize) {
    assert_eq!(a.len(), m * k, "dgemm_no_transpose: a.len() != m*k");
    assert_eq!(b.len(), k * n, "dgemm_no_transpose: b.len() != k*n");
    assert_eq!(c.len(), m * n, "dgemm_no_transpose: c.len() != m*n");

    #[cfg(not(target_os = "windows"))]
    unsafe {
        cblas::dgemm(
            cblas::Layout::RowMajor,
            cblas::Transpose::None,
            cblas::Transpose::None,
            m as i32,
            n as i32,
            k as i32,
            1.0,
            a,
            k as i32,
            b,
            n as i32,
            0.0,
            c,
            n as i32,
        );
    }

    #[cfg(target_os = "windows")]
    {
        for row in 0..m {
            for col in 0..n {
                let mut sum = 0.0f64;
                for kk in 0..k {
                    sum += a[row * k + kk] * b[kk * n + col];
                }
                c[row * n + col] = sum;
            }
        }
    }
}

/// How a spectral-upsampling LUT is consumed, from its `.toml` sidecar
/// (upstream's descriptor registry in `utils/spectral_upsampling.py`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LutKind {
    /// Light reaching the sensor, integrated directly (hanatos2025). The
    /// chromaticity projection uses the film's reference illuminant.
    Irradiance,
    /// Surface reflectance recovered under `scene_illuminant`, relit by the
    /// film's reference illuminant and normalised per channel on the
    /// method's own neutral. The projection uses the scene illuminant.
    Reflectance { scene_illuminant: &'static str },
}

/// A shipped spectral-upsampling method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpsamplingMethod {
    pub identifier: &'static str,
    pub file: &'static str,
    pub kind: LutKind,
}

/// The methods Negs ships: experimental 28bf883's default and its two GUI
/// options. The descriptors are hard-coded; a test checks them against the
/// `.toml` sidecars shipped next to each `.npy`.
pub const UPSAMPLING_METHODS: [UpsamplingMethod; 3] = [
    UpsamplingMethod {
        identifier: "hanatos2025",
        file: "hanatos2025_irradiance_xy_tc.npy",
        kind: LutKind::Irradiance,
    },
    UpsamplingMethod {
        identifier: "arctic2026beta04",
        file: "arctic2026beta04_reflectance_xy_tc.npy",
        kind: LutKind::Reflectance { scene_illuminant: "D65" },
    },
    UpsamplingMethod {
        identifier: "mallett2019",
        file: "mallett2019_reflectance_xy_tc.npy",
        kind: LutKind::Reflectance { scene_illuminant: "D65" },
    },
];

/// Look up a method by its `settings.rgb_to_raw_method` identifier.
///
/// `arctic2026alpha02`, which upstream deleted along with its LUT, resolves
/// to its successor `arctic2026beta04` so saved settings keep rendering.
pub fn upsampling_method(identifier: &str) -> Result<&'static UpsamplingMethod, String> {
    let identifier = match identifier {
        "arctic2026alpha02" => "arctic2026beta04",
        other => other,
    };
    UPSAMPLING_METHODS
        .iter()
        .find(|m| m.identifier == identifier)
        .ok_or_else(|| {
            let known: Vec<_> = UPSAMPLING_METHODS.iter().map(|m| m.identifier).collect();
            format!("unsupported rgb_to_raw_method: {identifier:?} (expected one of {known:?})")
        })
}

/// Load the hanatos2025 irradiance spectra LUT.
pub fn load_spectra_lut(data_dir: &Path) -> Result<SpectraLut, String> {
    load_method_lut(data_dir, &UPSAMPLING_METHODS[0])
}

/// Load a method's `(size, size, 81)` spectra LUT from
/// `<data_dir>/luts/spectral_upsampling/<file>` (float16 on disk, widened to
/// f32 exactly; numpy widens it to f64, also exactly).
pub fn load_method_lut(data_dir: &Path, method: &UpsamplingMethod) -> Result<SpectraLut, String> {
    let path = data_dir
        .join("luts")
        .join("spectral_upsampling")
        .join(method.file);
    let id = method.identifier;
    let file = std::fs::File::open(&path)
        .map_err(|e| format!("opening {id} spectra LUT {}: {e}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    let (shape, data) =
        npy::load_npy_f32(reader).map_err(|e| format!("loading {id} spectra LUT: {e}"))?;
    if shape.len() != 3 || shape[2] != N_WAVELENGTHS {
        return Err(format!(
            "{id} spectra LUT shape mismatch: expected (N, N, {N_WAVELENGTHS}), got {shape:?}"
        ));
    }
    if shape[0] != shape[1] {
        return Err(format!(
            "{id} spectra LUT must be square, got {}x{}",
            shape[0], shape[1]
        ));
    }
    Ok(SpectraLut {
        size: shape[0],
        n_wavelengths: shape[2],
        data,
    })
}

/// The spectrum a LUT emits for a neutral of chromaticity `xy`: the table
/// sampled at `_tri2quad(xy)` with the runtime's Mitchell cubic sampler
/// (upstream `apply_lut_cubic_2d(lut, white_tc)`).
pub fn neutral_spectrum(lut: &SpectraLut, xy: (f64, f64)) -> Vec<f64> {
    let (tx, ty) = spectral::xy_to_tc(xy.0, xy.1);
    let scale = (lut.size - 1) as f64;
    spektrafilm_math::lut::bicubic_2d_f32_as_f64(
        &lut.data,
        lut.size,
        lut.size,
        lut.n_wavelengths,
        tx * scale,
        ty * scale,
    )
}

pub struct SpectraLut {
    pub size: usize,
    pub n_wavelengths: usize,
    /// Flat: [size * size * n_wavelengths]
    pub data: Vec<f32>,
}

impl SpectraLut {
    /// Get spectrum at grid position (i, j). Returns slice of n_wavelengths.
    pub fn spectrum(&self, i: usize, j: usize) -> &[f32] {
        let start = (i * self.size + j) * self.n_wavelengths;
        &self.data[start..start + self.n_wavelengths]
    }
}

/// Compute the TC LUT for a given film stock.
///
/// Port of Python `compute_hanatos2025_tc_lut`:
///   tc_lut[i][j][c] = sum_wl( spectra_lut[i][j][wl] * sensitivity[wl][c] )
///
/// The result maps tc coordinates → per-channel film raw exposure,
/// normalized so that the reference illuminant midgray produces balanced
/// exposure on the green channel (matching Python's `raw / raw_midgray[1]`).
pub fn compute_tc_lut(spectra_lut: &SpectraLut, sensitivity: &[[f64; 3]]) -> TcLut {
    let size = spectra_lut.size;
    let n_wl = spectra_lut.n_wavelengths.min(sensitivity.len());
    let channels = 3;

    let mut data = vec![0.0f64; size * size * channels];

    for i in 0..size {
        for j in 0..size {
            let spectrum = spectra_lut.spectrum(i, j);
            let mut raw = [0.0f64; 3];
            for wl in 0..n_wl {
                for c in 0..3 {
                    raw[c] += spectrum[wl] as f64 * sensitivity[wl][c];
                }
            }
            let base = (i * size + j) * channels;
            data[base] = raw[0];
            data[base + 1] = raw[1];
            data[base + 2] = raw[2];
        }
    }

    TcLut {
        size,
        channels,
        data,
    }
}

/// TC → film-raw LUT for a reflectance method (upstream
/// `compute_reflectance_tc_lut`): relight each reflectance by the film's
/// reference illuminant, integrate against the sensitivity, then divide each
/// channel by the response to the method's own neutral, its reflectance at
/// the scene-illuminant chromaticity (`scene_xy`), relit the same way. A
/// neutral input, which the runtime projects under the scene illuminant,
/// lands on (1, 1, 1).
///
/// `raw[i,j,c] = sum_wl R[i,j,wl] E[wl] S[wl,c] / sum_wl R_n[wl] E[wl] S[wl,c]`
pub fn compute_reflectance_tc_lut(
    reflectance_lut: &SpectraLut,
    sensitivity: &[[f64; 3]],
    illuminant: &[f64],
    scene_xy: (f64, f64),
) -> TcLut {
    let size = reflectance_lut.size;
    let n_wl = reflectance_lut
        .n_wavelengths
        .min(sensitivity.len())
        .min(illuminant.len());
    let channels = 3;
    let n_pix = size * size;
    // `contract('ijl,lm->ijm', R * E, S)` goes through GEMM, as in the
    // windowed hanatos path.
    let stride = reflectance_lut.n_wavelengths;
    let mut relit = vec![0.0f64; n_pix * n_wl];
    for i in 0..n_pix {
        let src = &reflectance_lut.data[i * stride..i * stride + n_wl];
        let dst = &mut relit[i * n_wl..(i + 1) * n_wl];
        for ((d, &r), &e) in dst.iter_mut().zip(src).zip(illuminant) {
            *d = r as f64 * e;
        }
    }
    let mut sens_flat = vec![0.0f64; n_wl * 3];
    for wl in 0..n_wl {
        sens_flat[wl * 3..wl * 3 + 3].copy_from_slice(&sensitivity[wl]);
    }
    let mut data = vec![0.0f64; n_pix * channels];
    dgemm_no_transpose(&relit, &sens_flat, &mut data, n_pix, channels, n_wl);

    // `np.einsum('l,lm->m', neutral_refl * E_ref, sensitivity)`.
    let neutral = neutral_spectrum(reflectance_lut, scene_xy);
    let mut raw_neutral = [0.0f64; 3];
    for wl in 0..n_wl {
        let lit = neutral[wl] * illuminant[wl];
        for c in 0..3 {
            raw_neutral[c] += lit * sensitivity[wl][c];
        }
    }
    for px in data.chunks_exact_mut(3) {
        for c in 0..3 {
            px[c] /= raw_neutral[c];
        }
    }
    TcLut {
        size,
        channels,
        data,
    }
}

/// Compute the TC LUT with spectral bandpass window (erf4 model).
///
/// The window models the camera's UV/IR sensitivity cutoff. It's applied
/// to the sensitivity before integration, with normalization to preserve
/// white balance.
///
/// `window_params`: (c_uv, sigma_uv, c_ir, sigma_ir) — erf4 bandpass parameters.
/// `neutral`: the spectrum white balance is preserved for. Upstream
/// experimental (`e301791`) uses the neutral the LUT actually emits, the
/// cubic-sampled reconstruction at the reference illuminant's chromaticity
/// ([`neutral_spectrum`]), not the bare illuminant: under the film's non-CMF
/// sensitivities the two drift neutrals ~2-3% apart.
pub fn compute_tc_lut_with_window(
    spectra_lut: &SpectraLut,
    sensitivity: &[[f64; 3]],
    window_params: &[f64],
    neutral: &[f64],
) -> TcLut {
    let n_wl = spectra_lut
        .n_wavelengths
        .min(sensitivity.len())
        .min(N_WAVELENGTHS);

    // Compute erf4 bandpass window in f64
    let window = if window_params.len() >= 4 {
        eval_erf4_bandpass(window_params)
    } else {
        vec![[1.0f64; 3]; N_WAVELENGTHS]
    };

    // Window normalization — Python:
    //   norm_num = np.sum(sens * neutral_sd[:, None] * window, axis=0)
    //   norm_den = np.sum(sens * neutral_sd[:, None], axis=0)
    //   normalization = norm_num / norm_den
    //   window /= normalization
    // Multiplication order per cell: `(sens * neutral) * window`
    // — left-to-right. The 81-wavelength reduction in numpy uses
    // pairwise summation; for 81 elements that's recursive halving.
    // We replicate it via `pairwise_sum_f64`.
    let mut num_per_wl = [
        Vec::<f64>::with_capacity(n_wl),
        Vec::with_capacity(n_wl),
        Vec::with_capacity(n_wl),
    ];
    let mut den_per_wl = [
        Vec::<f64>::with_capacity(n_wl),
        Vec::with_capacity(n_wl),
        Vec::with_capacity(n_wl),
    ];
    for wl in 0..n_wl {
        for c in 0..3 {
            let si = sensitivity[wl][c] * neutral[wl];
            num_per_wl[c].push(si * window[wl][c]);
            den_per_wl[c].push(si);
        }
    }
    let mut window_normalized = window.clone();
    for c in 0..3 {
        let num_c = pairwise_sum_f64(&num_per_wl[c]);
        let den_c = pairwise_sum_f64(&den_per_wl[c]);
        if num_c > 1e-10 && den_c > 1e-10 {
            let normalization_c = num_c / den_c;
            for wl in 0..n_wl {
                window_normalized[wl][c] = window[wl][c] / normalization_c;
            }
        }
    }

    // Compute raw LUT via BLAS dgemm to match Python's
    // `opt_einsum.contract('ijl,lm->ijm', spectra, sens*window)`.
    // numpy/opt_einsum routes this through GEMM with pairwise
    // accumulation, so a hand-rolled left-to-right loop is off by
    // 1-2 ULP per cell. Going through dgemm matches bit-for-bit.
    let size = spectra_lut.size;
    let channels = 3;
    let n_pix = size * size;
    let mut spec_f64 = vec![0.0f64; n_pix * n_wl];
    for i in 0..n_pix {
        let src =
            &spectra_lut.data[i * spectra_lut.n_wavelengths..i * spectra_lut.n_wavelengths + n_wl];
        let dst = &mut spec_f64[i * n_wl..(i + 1) * n_wl];
        for (d, s) in dst.iter_mut().zip(src.iter()) {
            *d = *s as f64;
        }
    }
    // Build sens*window as (n_wl × 3) row-major.
    let mut sw_flat = vec![0.0f64; n_wl * 3];
    for wl in 0..n_wl {
        for c in 0..3 {
            sw_flat[wl * 3 + c] = sensitivity[wl][c] * window_normalized[wl][c];
        }
    }
    let mut data = vec![0.0f64; n_pix * channels];
    dgemm_no_transpose(&spec_f64, &sw_flat, &mut data, n_pix, channels, n_wl);

    TcLut {
        size,
        channels,
        data,
    }
}

/// Compute the midgray normalization factor for Hanatos2025.
///
/// Port of Python: `raw_midgray = einsum('k,km->m', illuminant * 0.184, sensitivity)`
/// then normalize by `raw_midgray[1]` (green channel).
pub fn compute_midgray_normalization(sensitivity: &[[f64; 3]], illuminant: &[f32]) -> f64 {
    let n_wl = sensitivity.len().min(illuminant.len());
    let mut raw_midgray = [0.0f64; 3];
    for wl in 0..n_wl {
        for c in 0..3 {
            raw_midgray[c] += illuminant[wl] as f64 * 0.184 * sensitivity[wl][c];
        }
    }
    if raw_midgray[1] > 1e-10 {
        1.0 / raw_midgray[1]
    } else {
        1.0
    }
}

/// Public re-export: pairwise sum for f64 spectra/wavelength reductions.
/// See `pairwise_sum_f64` doc below.
pub fn pairwise_sum_f64_pub(xs: &[f64]) -> f64 {
    pairwise_sum_f64(xs)
}

/// Pairwise (recursive-halving) f64 summation. Matches numpy's
/// `np.add.reduce` reduction pattern, which is what `np.sum` uses for
/// 1-D arrays. For 81 elements this is recursive halving; the result
/// differs from a naive left-to-right sum by 1-2 ULPs but matches numpy
/// bit-for-bit.
fn pairwise_sum_f64(xs: &[f64]) -> f64 {
    match xs.len() {
        0 => 0.0,
        1 => xs[0],
        2 => xs[0] + xs[1],
        n => {
            let mid = n / 2;
            pairwise_sum_f64(&xs[..mid]) + pairwise_sum_f64(&xs[mid..])
        }
    }
}

/// Evaluate the erf4 spectral bandpass window.
/// params: (c_uv, sigma_uv, c_ir, sigma_ir)
/// Returns [81][3] window values (same for all 3 channels in erf4 model).
fn eval_erf4_bandpass(params: &[f64]) -> Vec<[f64; 3]> {
    let sqrt2 = std::f64::consts::SQRT_2;
    let c_uv = params[0];
    let sigma_uv = params[1];
    let c_ir = params[2];
    let sigma_ir = params[3];

    let mut window = vec![[0.0f64; 3]; N_WAVELENGTHS];
    for i in 0..N_WAVELENGTHS {
        let wl = spectral::WAVELENGTH_MIN as f64 + (i as f64) * spectral::WAVELENGTH_STEP as f64;
        let edge_uv = 0.5 * (1.0 + erf((wl - c_uv) / (sigma_uv * sqrt2)));
        let edge_ir = 0.5 * (1.0 - erf((wl - c_ir) / (sigma_ir * sqrt2)));
        let w = edge_uv * edge_ir;
        window[i] = [w, w, w];
    }
    window
}

/// f64 erf via libm — matches scipy.special.erf at full f64 precision.
#[inline]
fn erf(x: f64) -> f64 {
    libm::erf(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    /// `key = "value"` from a sidecar, scoped to `[section]` ("" = top level).
    /// A minimal reader: the sidecars only need quoted strings here.
    fn toml_string(text: &str, section: &str, key: &str) -> Option<String> {
        let mut current = String::new();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                current = name.trim().to_string();
            } else if current == section {
                if let Some((k, v)) = line.split_once('=') {
                    if k.trim() == key {
                        return Some(v.trim().trim_matches('"').to_string());
                    }
                }
            }
        }
        None
    }

    /// The hard-coded descriptors agree with the shipped `.toml` sidecars,
    /// and every LUT loads at the declared size.
    #[test]
    fn method_descriptors_match_toml_sidecars() {
        let dir = data_dir().join("luts").join("spectral_upsampling");
        for method in &UPSAMPLING_METHODS {
            let sidecar = dir.join(method.file.replace(".npy", ".toml"));
            let text = std::fs::read_to_string(&sidecar).unwrap();
            let get = |section, key| toml_string(&text, section, key);
            assert_eq!(get("", "identifier").as_deref(), Some(method.identifier));
            assert_eq!(get("", "file").as_deref(), Some(method.file));
            match method.kind {
                LutKind::Irradiance => {
                    assert_eq!(get("", "kind").as_deref(), Some("irradiance"));
                    assert_eq!(get("reflectance", "scene_illuminant"), None);
                }
                LutKind::Reflectance { scene_illuminant } => {
                    assert_eq!(get("", "kind").as_deref(), Some("reflectance"));
                    assert_eq!(
                        get("reflectance", "scene_illuminant").as_deref(),
                        Some(scene_illuminant)
                    );
                }
            }
            let lut = load_method_lut(&data_dir(), method).unwrap();
            let size: usize = get("array", "lut_size").unwrap().parse().unwrap();
            assert_eq!((lut.size, lut.n_wavelengths), (size, N_WAVELENGTHS));
        }
    }

    #[test]
    fn method_lookup_aliases_the_deleted_alpha02() {
        assert_eq!(
            upsampling_method("arctic2026alpha02").unwrap().identifier,
            "arctic2026beta04"
        );
        assert!(upsampling_method("jakob2019").is_err());
    }
}
