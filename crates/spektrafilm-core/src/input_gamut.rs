//! Input gamut compression — faithful port of upstream
//! `spektrafilm/utils/gamut_compression.py` (input side).
//!
//! The compression is baked into the tc_lut once at build time via
//! remap-resample: `new_lut[xy] = old_lut[compress(xy)]`. The per-pixel
//! runtime path (RGB → CIE xy → tc → bilinear LUT sample) therefore stays
//! compression-agnostic. Only the production `"xy"` algorithm (ACES-RGC-style
//! radial compression toward the visible spectral locus) is ported, including
//! experimental 28bf883's optional smooth inscribed hull.

use rustfft::{FftPlanner, num_complex::Complex};
use std::sync::{Arc, Mutex};
use spektrafilm_math::spectral::{self, CMF_X_F64, CMF_Y_F64, CMF_Z_F64, TcLut};

use crate::gamut_compression::reinhard_knee;
use crate::params::InputGamutCompressParams;

/// CIE 1931 2° visible spectral locus as a closed xy polygon, sampled at 5 nm
/// from 380 to 700 nm (65 vertices + the first repeated). Mirrors upstream
/// `spectral_locus_xy()`; the CMFs are the same colour-science values.
fn spectral_locus_xy() -> Vec<[f64; 2]> {
    // Wavelengths 380..=700 step 5 → indices 0..65 of the 380..780 grid.
    const N: usize = 65;
    let mut poly = Vec::with_capacity(N + 1);
    for i in 0..N {
        let (x, y, z) = (CMF_X_F64[i], CMF_Y_F64[i], CMF_Z_F64[i]);
        let total = (x + y + z).max(1e-12);
        poly.push([x / total, y / total]);
    }
    poly.push(poly[0]);
    poly
}

/// Distance from `origin` along unit `direction` to the first intersection
/// with the closed polygon, via parametric segment intersection. Returns
/// `f64::INFINITY` for rays that miss (should not happen for an interior
/// origin and the visible locus). Mirrors upstream `_ray_polygon_distance`.
fn ray_polygon_distance(origin: [f64; 2], direction: [f64; 2], polygon: &[[f64; 2]]) -> f64 {
    let mut t_min = f64::INFINITY;
    let (dx, dy) = (direction[0], direction[1]);
    for seg in polygon.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
        let denom = dx * ey - dy * ex;
        if denom.abs() <= 1e-12 {
            continue;
        }
        let (ox, oy) = (origin[0] - a[0], origin[1] - a[1]);
        // origin + t·direction = a + s·edge
        let t = (-ox * ey + oy * ex) / denom;
        let s = (-ox * dy + oy * dx) / denom;
        // Match experimental's endpoint guard: dense hull vertices can fall
        // exactly on a ray, with s a rounding error outside the adjacent edge.
        if t > 1e-9 && (-1e-9..=1.0 + 1e-9).contains(&s) && t < t_min {
            t_min = t;
        }
    }
    t_min
}

/// Sample radial reach at 1024 angles, Gaussian-filter its Fourier modes,
/// then scale by the worst overshoot to inscribe the smoothed boundary.
/// Mirrors `inscribed_locus_hull` at pinned Python experimental 28bf883.
fn inscribed_locus_hull(white: [f64; 2], detail: f64, locus: &[[f64; 2]]) -> Vec<[f64; 2]> {
    const N: usize = 1024;
    let directions: Vec<_> = (0..N)
        .map(|i| {
            let theta = i as f64 * (std::f64::consts::TAU / N as f64);
            [theta.cos(), theta.sin()]
        })
        .collect();
    let reach: Vec<_> = directions
        .iter()
        .map(|&dir| ray_polygon_distance(white, dir, locus))
        .collect();
    let mut modes: Vec<_> = reach.iter().map(|&r| Complex::new(r, 0.0)).collect();
    let mut planner = FftPlanner::<f64>::new();
    planner.plan_fft_forward(N).process(&mut modes);
    for (k, mode) in modes.iter_mut().enumerate() {
        let frequency = k.min(N - k) as f64;
        *mode *= (-0.5 * (frequency / detail).powi(2)).exp();
    }
    planner.plan_fft_inverse(N).process(&mut modes);
    let smoothed: Vec<_> = modes.iter().map(|m| (m.re / N as f64).max(1e-12)).collect();
    let scale = reach
        .iter()
        .zip(&smoothed)
        .map(|(r, s)| r / s)
        .fold(f64::INFINITY, f64::min);
    let mut hull: Vec<_> = directions
        .iter()
        .zip(&smoothed)
        .map(|(dir, r)| [white[0] + r * scale * dir[0], white[1] + r * scale * dir[1]])
        .collect();
    hull.push(hull[0]);
    hull
}

/// ACES-RGC-style radial compression of a single CIE xy toward the spectral
/// locus, around `white_xy`. Hue (dominant wavelength) is preserved by
/// construction. Mirrors upstream `compress_xy_radial`.
fn compress_xy_radial(
    xy: [f64; 2],
    white_xy: [f64; 2],
    knee: (f64, f64, f64),
    locus: &[[f64; 2]],
) -> [f64; 2] {
    let delta = [xy[0] - white_xy[0], xy[1] - white_xy[1]];
    let dist = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
    if dist < 1e-9 {
        return xy;
    }
    let safe_dist = dist.max(1e-12);
    let direction = [delta[0] / safe_dist, delta[1] / safe_dist];
    let boundary = ray_polygon_distance(white_xy, direction, locus);
    let d_norm = dist / boundary.max(1e-12);
    let (t, l, p) = knee;
    let d_compressed = reinhard_knee(d_norm, t, l, p);
    let scaled = d_compressed * boundary;
    [
        white_xy[0] + direction[0] * scaled,
        white_xy[1] + direction[1] * scaled,
    ]
}

/// Configured input gamut compressor. `build` resolves the algorithm string;
/// `is_active` reports whether a non-identity remap will be applied.
pub struct InputGamutCompress {
    active: bool,
    knee: (f64, f64, f64),
    locus: Vec<[f64; 2]>,
    hull_detail: Option<f64>,
    center_d65: bool,
}

impl InputGamutCompress {
    /// Resolve from the param algorithm string. `"off"` → inactive (identity);
    /// `"xy"` → active radial compression. Any other value errors. The knee is
    /// validated as in upstream `InputGamutCompressSpec.__post_init__`. The
    /// boundary is the raw locus centred on the given white, upstream main's.
    pub fn build(algorithm: &str, knee: [f32; 3]) -> Result<Self, String> {
        Self::from_params(&InputGamutCompressParams {
            algorithm: algorithm.into(),
            knee,
            boundary: "locus".into(),
            center: "reference".into(),
            ..Default::default()
        })
    }

    /// Resolve the legacy locus or experimental inscribed-hull boundary.
    pub fn from_params(params: &InputGamutCompressParams) -> Result<Self, String> {
        let algorithm = params.algorithm.as_str();
        let knee = params.knee;
        let active = match algorithm {
            "off" => false,
            "xy" => true,
            other => return Err(format!("unknown input gamut algorithm: {other}")),
        };
        let hull_detail = match params.boundary.as_str() {
            "locus" => None,
            "inscribed_hull" => Some(params.hull_detail),
            other => return Err(format!("unknown input gamut boundary: {other}")),
        };
        let center_d65 = match params.center.as_str() {
            "reference" => false,
            "D65" => true,
            other => return Err(format!("unknown input gamut center: {other}")),
        };
        if !params.hull_detail.is_finite() || params.hull_detail <= 0.0 {
            return Err("hull_detail must be positive and finite".into());
        }
        let [t, l, p] = knee;
        if !(0.0..1.0).contains(&t) {
            return Err(format!("knee threshold must be in [0, 1), got {t}"));
        }
        if !l.is_finite() || l <= 0.0 {
            return Err(format!("knee limit must be > 0, got {l}"));
        }
        if !p.is_finite() || p <= 0.0 {
            return Err(format!("knee power must be > 0, got {p}"));
        }
        Ok(Self {
            active,
            hull_detail,
            center_d65,
            knee: (knee[0] as f64, knee[1] as f64, knee[2] as f64),
            locus: if active {
                spectral_locus_xy()
            } else {
                Vec::new()
            },
        })
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Bake the compression into a freshly resampled copy of `lut`. For each
    /// LUT cell we decode its tc index to CIE xy, compress, re-encode to tc,
    /// and bilinearly sample the original LUT there (clamp-to-edge boundary,
    /// matching `scipy.ndimage.map_coordinates(order=1, mode="nearest")`).
    /// `ref_xy` is the upsampler's reference-white chromaticity. An explicit
    /// D65 center overrides it, independently of film spectral adaptation.
    /// Returns `lut` unchanged when inactive.
    pub fn remap(&self, lut: &TcLut, ref_xy: [f64; 2]) -> TcLut {
        if !self.active {
            return lut.clone();
        }
        let ref_xy = if self.center_d65 {
            let (x, y) = spectral::illuminant_to_xy(&spectral::ILLUMINANT_D65);
            [x, y]
        } else {
            ref_xy
        };
        let size = lut.size;
        let ch = lut.channels;
        let coords = self.sample_coords(ref_xy, size);
        let mut data = vec![0.0f64; size * size * ch];
        // Step 4: bilinear sample original LUT at (tx, ty) grid coords.
        for (cell, &(ci, cj)) in data.chunks_exact_mut(ch).zip(coords.iter()) {
            let sample = bilinear_sample(lut, ci, cj);
            cell.copy_from_slice(&sample[..ch]);
        }
        TcLut {
            size,
            channels: ch,
            data,
        }
    }

    /// Steps 1–3 of [`Self::remap`]: the grid coordinates each cell of a
    /// `size`² LUT samples. They don't depend on the LUT (the film), only on
    /// the white and the compressor, so every film with the same reference
    /// white shares them: kept for the process, ~0.6 MB each at 192², and the
    /// 62 stocks have 4 whites. The key holds every input exactly (bits); the
    /// locus is a constant. Ticket 29: a film change built this in ~11 ms.
    fn sample_coords(&self, ref_xy: [f64; 2], size: usize) -> Arc<Vec<(f64, f64)>> {
        type Key = ([u64; 2], [u64; 3], Option<u64>, usize);
        static CACHE: Mutex<Vec<(Key, Arc<Vec<(f64, f64)>>)>> = Mutex::new(Vec::new());
        const ENTRIES: usize = 8;
        let key: Key = (
            ref_xy.map(f64::to_bits),
            [self.knee.0.to_bits(), self.knee.1.to_bits(), self.knee.2.to_bits()],
            self.hull_detail.map(f64::to_bits),
            size,
        );
        if let Some((_, coords)) = CACHE.lock().unwrap().iter().find(|(k, _)| *k == key) {
            #[cfg(test)]
            tests::CACHE_HITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return coords.clone();
        }
        let hull = self
            .hull_detail
            .map(|detail| inscribed_locus_hull(ref_xy, detail, &self.locus));
        let boundary = hull.as_deref().unwrap_or(&self.locus);
        let inv = 1.0 / (size as f64 - 1.0);
        let mut coords = vec![(0.0, 0.0); size * size];
        // Each cell is independent; rows in parallel.
        use rayon::prelude::*;
        coords.par_chunks_mut(size).enumerate().for_each(|(i, row)| {
            for (j, slot) in row.iter_mut().enumerate() {
                // Step 1: tc cell → CIE xy.
                let (x, y) = spectral::tc_to_xy(i as f64 * inv, j as f64 * inv);
                // Step 2: compress.
                let cxy = compress_xy_radial([x, y], ref_xy, self.knee, boundary);
                // Step 3: compressed xy → tc, as grid coordinates.
                let (tx, ty) = spectral::xy_to_tc(cxy[0], cxy[1]);
                *slot = (tx * (size as f64 - 1.0), ty * (size as f64 - 1.0));
            }
        });
        let coords = Arc::new(coords);
        let mut cache = CACHE.lock().unwrap();
        if cache.len() >= ENTRIES {
            cache.remove(0);
        }
        cache.push((key, coords.clone()));
        coords
    }
}

/// Bilinear sample of all channels at fractional grid coordinate `(ci, cj)`,
/// clamping fetch indices to the edge (scipy `mode="nearest"`).
fn bilinear_sample(lut: &TcLut, ci: f64, cj: f64) -> [f64; 3] {
    let size = lut.size;
    let ch = lut.channels;
    let max_idx = size as isize - 1;
    let clamp = |v: isize| v.clamp(0, max_idx) as usize;

    let bi = ci.floor();
    let bj = cj.floor();
    let fi = ci - bi;
    let fj = cj - bj;
    let (i0, i1) = (clamp(bi as isize), clamp(bi as isize + 1));
    let (j0, j1) = (clamp(bj as isize), clamp(bj as isize + 1));

    let cell = |i: usize, j: usize, c: usize| lut.data[(i * size + j) * ch + c];
    let mut out = [0.0f64; 3];
    for (c, slot) in out.iter_mut().enumerate().take(ch) {
        let v00 = cell(i0, j0, c);
        let v01 = cell(i0, j1, c);
        let v10 = cell(i1, j0, c);
        let v11 = cell(i1, j1, c);
        *slot = v00 * (1.0 - fi) * (1.0 - fj)
            + v01 * (1.0 - fi) * fj
            + v10 * fi * (1.0 - fj)
            + v11 * fi * fj;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// Cache hits in `sample_coords`, so the bit-identity test can show it
    /// compared cached results, not only fresh ones.
    pub(super) static CACHE_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    #[derive(Deserialize)]
    struct HullFixture {
        input_cases: Vec<HullCase>,
    }

    #[derive(Deserialize)]
    struct HullCase {
        white: [f64; 2],
        spec: InputGamutCompressParams,
        hull_every_64: Vec<[f64; 2]>,
        remapped_lut: Vec<f64>,
    }

    #[test]
    fn experimental_hull_and_remap_match_python() {
        let fixture: HullFixture =
            serde_json::from_str(include_str!("../tests/fixtures/gamut_python_28bf883.json"))
                .unwrap();
        let locus = spectral_locus_xy();
        for case in fixture.input_cases {
            let hull = inscribed_locus_hull(case.white, case.spec.hull_detail, &locus);
            assert_eq!(hull.len(), 1025);
            assert_eq!(hull.first(), hull.last());
            for (actual, expected) in hull.iter().step_by(64).zip(case.hull_every_64) {
                for c in 0..2 {
                    assert!((actual[c] - expected[c]).abs() < 1e-11);
                }
            }
            // Every hull vertex remains inside the locus; rays aligned with
            // vertices must intersect rather than produce NaNs in compression.
            for &vertex in &hull[..1024] {
                let delta = [vertex[0] - case.white[0], vertex[1] - case.white[1]];
                let radius = delta[0].hypot(delta[1]);
                let direction = [delta[0] / radius, delta[1] / radius];
                assert!(radius <= ray_polygon_distance(case.white, direction, &locus) + 1e-11);
                assert!(
                    (ray_polygon_distance(case.white, direction, &hull) - radius).abs() < 1e-11
                );
            }
            // Python's remap centres on the white it is given.
            let spec = InputGamutCompressParams { center: "reference".into(), ..case.spec.clone() };
            let compressor = InputGamutCompress::from_params(&spec).unwrap();
            let actual = compressor.remap(&synthetic_lut(16), case.white);
            assert_eq!(actual.data.len(), case.remapped_lut.len());
            for (i, (a, e)) in actual.data.iter().zip(&case.remapped_lut).enumerate() {
                assert!(
                    (a - e).abs() < 1e-6,
                    "detail {} component {i}: {a} vs {e}",
                    case.spec.hull_detail
                );
            }
            assert_eq!(
                compress_xy_radial(case.white, case.white, compressor.knee, &hull),
                case.white
            );
        }
    }

    #[test]
    fn boundary_configuration_parses_main_settings_and_rejects_invalid_values() {
        let legacy: InputGamutCompressParams = serde_json::from_str(
            r#"{"algorithm":"xy","knee":[0,1,6],"boundary":"locus","center":"reference"}"#,
        )
        .unwrap();
        assert!(InputGamutCompress::from_params(&legacy).is_ok());
        for detail in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let params = InputGamutCompressParams {
                boundary: "inscribed_hull".into(),
                hull_detail: detail,
                ..legacy.clone()
            };
            assert!(InputGamutCompress::from_params(&params).is_err());
        }
        let params = InputGamutCompressParams {
            boundary: "unknown".into(),
            ..legacy
        };
        assert!(InputGamutCompress::from_params(&params).is_err());
        for knee in [
            [f32::NAN, 1.0, 6.0],
            [0.0, f32::NAN, 6.0],
            [0.0, 1.0, f32::INFINITY],
        ] {
            assert!(InputGamutCompress::build("xy", knee).is_err());
        }
    }

    /// Deterministic synthetic LUT, mirrored bit-for-bit in the Python
    /// reference (`tmp/igc_ref.py`): cell (i, j, c) = `((i² + 3j + 7c) % 97) / 97`.
    fn synthetic_lut(size: usize) -> TcLut {
        let mut data = vec![0.0f64; size * size * 3];
        for i in 0..size {
            for j in 0..size {
                for c in 0..3 {
                    let v = ((i * i + j * 3 + c * 7) % 97) as f64 / 97.0;
                    data[(i * size + j) * 3 + c] = v;
                }
            }
        }
        TcLut {
            size,
            channels: 3,
            data,
        }
    }

    /// The baked tc_lut remap matches the upstream remap-resample
    /// (`remap_tc_lut_for_compression` with the `"xy"` algorithm) against the
    /// CIE 1931 2° locus, sampled with scipy `map_coordinates(order=1,
    /// mode="nearest")`.
    #[test]
    fn remap_matches_python_reference() {
        let spec = InputGamutCompress::build("xy", [0.0, 1.0, 6.0]).unwrap();
        let lut = synthetic_lut(64);
        let out = spec.remap(&lut, [0.3127, 0.3290]);

        // (i, j) → [r, g, b] from the Python reference.
        let cells = [
            (0usize, 0usize, [0.4137658005, 0.1877218965, 0.2598868450]),
            (10, 20, [0.7521338372, 0.8242987857, 0.8964637341]),
            (32, 32, [0.5463917966, 0.6185567450, 0.6907215587]),
            (50, 5, [0.5807479510, 0.0691153288, 0.1412802772]),
            (63, 63, [0.6891891640, 0.7613541125, 0.8335190609]),
            (5, 60, [0.0995187055, 0.1716836540, 0.2438486024]),
        ];
        for (i, j, expected) in cells {
            let base = (i * 64 + j) * 3;
            for c in 0..3 {
                assert!(
                    (out.data[base + c] - expected[c]).abs() < 1e-7,
                    "cell ({i},{j}) ch {c}: got {}, expected {}",
                    out.data[base + c],
                    expected[c],
                );
            }
        }

        let checksum: f64 = out.data.iter().sum();
        assert!(
            (checksum - 5846.5426164588).abs() < 1e-6,
            "checksum {checksum}",
        );
    }

    /// An inactive ("off") compressor returns the LUT unchanged.
    #[test]
    fn off_is_identity() {
        let spec = InputGamutCompress::build("off", [0.0, 1.0, 6.0]).unwrap();
        assert!(!spec.is_active());
        let lut = synthetic_lut(16);
        let out = spec.remap(&lut, [0.3127, 0.3290]);
        assert_eq!(out.data, lut.data);
    }

    /// `remap` before ticket 29 (main 5a1fa3c), verbatim but for the name:
    /// every cell computed in one pass, nothing cached.
    fn remap_uncached(g: &InputGamutCompress, lut: &TcLut, ref_xy: [f64; 2]) -> TcLut {
        if !g.active {
            return lut.clone();
        }
        let ref_xy = if g.center_d65 {
            let (x, y) = spectral::illuminant_to_xy(&spectral::ILLUMINANT_D65);
            [x, y]
        } else {
            ref_xy
        };
        let hull = g
            .hull_detail
            .map(|detail| inscribed_locus_hull(ref_xy, detail, &g.locus));
        let boundary = hull.as_deref().unwrap_or(&g.locus);
        let size = lut.size;
        let ch = lut.channels;
        let inv = 1.0 / (size as f64 - 1.0);
        let mut data = vec![0.0f64; size * size * ch];
        use rayon::prelude::*;
        data.par_chunks_mut(size * ch).enumerate().for_each(|(i, row)| {
            for j in 0..size {
                let (x, y) = spectral::tc_to_xy(i as f64 * inv, j as f64 * inv);
                let cxy = compress_xy_radial([x, y], ref_xy, g.knee, boundary);
                let (tx, ty) = spectral::xy_to_tc(cxy[0], cxy[1]);
                let sample =
                    bilinear_sample(lut, tx * (size as f64 - 1.0), ty * (size as f64 - 1.0));
                row[j * ch..j * ch + ch].copy_from_slice(&sample[..ch]);
            }
        });
        TcLut {
            size,
            channels: ch,
            data,
        }
    }

    /// Ticket 29: the cached remap is the uncached one, bit for bit, for real
    /// film TC LUTs, every white the stocks use, every boundary and center,
    /// random knees and hull details, other LUT sizes, and enough distinct
    /// keys in a shuffled order to hit, miss and evict the cache many times.
    #[test]
    fn cached_remap_is_bit_identical_to_uncached() {
        check_cached_remap(&["kodak_portra_400", "kodak_aerochrome_1443"]);
    }

    /// The same with more films: `cargo test --release -p spektrafilm-core
    /// --lib cached_remap -- --ignored` (~10–40 s).
    #[test]
    #[ignore]
    fn cached_remap_is_bit_identical_to_uncached_more_films() {
        check_cached_remap(&["kodak_portra_400", "kodak_vision3_500t", "fujifilm_velvia_50", "kodak_aerochrome_1443"]);
    }

    fn check_cached_remap(films: &[&str]) {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let method = crate::spectral_service::upsampling_method("hanatos2025").unwrap();
        let spectra = crate::spectral_service::load_method_lut(&data, method).unwrap();
        // A few real film LUTs (different sensitivities) and every white.
        let mut luts = Vec::new();
        for film in films {
            let profile = crate::profile::load_profile_by_name(&data, film).unwrap();
            let sens: Vec<[f64; 3]> = profile
                .log_sensitivity_f64()
                .iter()
                .map(|row| row.map(|v| { let x = 10f64.powf(v); if x.is_nan() { 0.0 } else { x } }))
                .collect();
            luts.push(crate::spectral_service::compute_tc_lut(&spectra, &sens));
        }
        // Small synthetic LUTs for other sizes (edge rows, odd sizes).
        let mut state = 0x9e3779b97f4a7c15u64;
        let mut rand = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        for size in [2usize, 17, 64] {
            let data = (0..size * size * 3).map(|_| rand() * 4.0).collect();
            luts.push(TcLut { size, channels: 3, data });
        }
        let mut whites: Vec<[f64; 2]> = ["D55", "D65", "D50", "BB3200", "TH-KG3", "T"]
            .iter()
            .map(|name| {
                let (x, y) = spectral::illuminant_to_xy(&crate::illuminants::select_illuminant(name));
                [x, y]
            })
            .collect();
        whites.push([0.3324, 0.3474]);
        let mut settings = Vec::new();
        for boundary in ["inscribed_hull", "locus"] {
            for center in ["reference", "D65"] {
                settings.push(InputGamutCompressParams { boundary: boundary.into(), center: center.into(), ..Default::default() });
            }
        }
        for _ in 0..6 {
            settings.push(InputGamutCompressParams {
                knee: [(rand() * 0.95) as f32, (0.2 + rand() * 2.0) as f32, (0.3 + rand() * 3.0) as f32],
                hull_detail: 1.0 + rand() * 40.0,
                ..Default::default()
            });
        }
        // Each key part alone: knees with the default hull, hull details with
        // the default knee (a key that drops either part must fail here).
        for _ in 0..3 {
            settings.push(InputGamutCompressParams {
                knee: [(rand() * 0.95) as f32, (0.2 + rand() * 2.0) as f32, (0.3 + rand() * 3.0) as f32],
                ..Default::default()
            });
            settings.push(InputGamutCompressParams { hull_detail: 1.0 + rand() * 40.0, ..Default::default() });
        }
        settings.push(InputGamutCompressParams { algorithm: "off".into(), ..Default::default() });
        let mut cases = Vec::new();
        for (l, _) in luts.iter().enumerate() {
            for (w, _) in whites.iter().enumerate() {
                for (p, _) in settings.iter().enumerate() {
                    cases.push((l, w, p));
                }
            }
        }
        // In order, so the four film LUTs share each key (a miss, then hits),
        // then shuffled, so keys are evicted and rebuilt in any order.
        let mut shuffled = cases.clone();
        for i in (1..shuffled.len()).rev() {
            shuffled.swap(i, (rand() * (i + 1) as f64) as usize);
        }
        let mut grouped = cases.clone();
        grouped.sort_by_key(|&(l, w, p)| (w, p, l));
        let order: Vec<_> = grouped.into_iter().chain(shuffled).collect();
        let hits_before = CACHE_HITS.load(std::sync::atomic::Ordering::Relaxed);
        let mut compared = 0usize;
        for (l, w, p) in order {
            let g = InputGamutCompress::from_params(&settings[p]).unwrap();
            let (lut, white) = (&luts[l], whites[w]);
            let want = remap_uncached(&g, lut, white);
            let got = g.remap(lut, white);
            assert_eq!((got.size, got.channels), (want.size, want.channels));
            let same = got.data.iter().zip(&want.data).all(|(a, b)| a.to_bits() == b.to_bits());
            assert!(same, "lut {l} white {white:?} settings {:?}", settings[p]);
            compared += got.data.len();
        }
        let hits = CACHE_HITS.load(std::sync::atomic::Ordering::Relaxed) - hits_before;
        // 7 whites × 16 active settings, hit by every film LUT but the first.
        assert!(hits >= 7 * 16 * (films.len() - 1), "only {hits} cache hits");
        eprintln!("compared {compared} values in {} remaps, {hits} cache hits", 2 * cases.len());
    }
}
