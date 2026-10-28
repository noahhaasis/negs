//! New source-based materials: independent Python parity and physical-paper invariants.
use spektrafilm_core::{params::{RuntimeParams, SpectralBoundary}, pipeline::Pipeline, profile};
use spektrafilm_gpu::cpu_backend::{CpuBackend, scan_log_xyz_cpu};
use spektrafilm_math::{image::ImageBuf, precision::from_f64, spectral};
use std::path::PathBuf;

const FILM: &str = "kodak_trix400_d76_8min";
const PAPER: &str = "ilford_mgrc_deluxe_grade2";

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn params() -> RuntimeParams {
    serde_json::from_str(include_str!(
        "../../../docs/bw-expansion-review/recipes/trix-paper.json"
    ))
    .map(|mut p: RuntimeParams| {
        p.io.input_cctf_decoding = false;
        p.io.output_cctf_encoding = false;
        p
    })
    .unwrap()
}

/// Linear sRGB whose XYZ is comfortably inside the xy triangle (every
/// component at least 2% of X + Y + Z), so the front pass's adaptation to
/// the film's reference illuminant can't move it across the boundary.
fn clearly_physical(rgb: &[f64]) -> bool {
    let m = spektrafilm_math::colorspace::RgbColourspace::output("sRGB").rgb_to_xyz;
    let xyz = spektrafilm_math::colorspace::mat3_vec_f64(&m, [rgb[0], rgb[1], rgb[2]]);
    let b = xyz[0] + xyz[1] + xyz[2];
    b > 0.0 && xyz.iter().all(|&v| v >= 0.02 * b)
}

#[test]
fn new_materials_match_independent_python_extremes_and_colours() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/bw_expansion_python.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        for (lut, boundary) in [false, true].into_iter().flat_map(|lut| [(lut, SpectralBoundary::Guard), (lut, SpectralBoundary::Upstream)]) {
            let mut p: RuntimeParams = serde_json::from_value(case["params"].clone()).unwrap();
            p.settings.use_enlarger_lut = lut;
            p.settings.use_scanner_lut = lut;
            p.settings.spectral_boundary = boundary;
            let pipeline = Pipeline::new_with_spectral(
                profile::load_profile_by_name(&data_dir(), FILM).unwrap(),
                profile::load_profile_by_name(&data_dir(), PAPER).unwrap(),
                p,
                &data_dir(),
            )
            .unwrap();
            let input = ImageBuf::from_data(
                case["width"].as_u64().unwrap() as u32,
                case["height"].as_u64().unwrap() as u32,
                case["input_rgb"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| from_f64(v.as_f64().unwrap()))
                    .collect(),
            );
            let out = pipeline.process(input, &CpuBackend);
            assert_eq!(
                out.data.len(),
                case["expected_rgb"].as_array().unwrap().len()
            );
            #[cfg(feature = "precision-f64")]
            let tolerance = 1e-6;
            #[cfg(not(feature = "precision-f64"))]
            let tolerance = 3e-3;
            let rgb: Vec<f64> = case["input_rgb"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
            for (i, (&got, expected)) in out
                .data
                .iter()
                .zip(case["expected_rgb"].as_array().unwrap())
                .enumerate()
            {
                assert!(got.is_finite(), "{} LUT={lut}: component {i} is {got}", case["name"]);
                // Upstream folds chromaticities beyond the xy triangle back
                // inside; the spectral boundary guard moves them toward white
                // instead (see `adapted_xyz_to_tc_b`). Pixels are independent
                // here (no spatial effects or auto exposure), so with the
                // guard every clearly physical pixel must still match.
                if boundary == SpectralBoundary::Guard && !clearly_physical(&rgb[i / 3 * 3..i / 3 * 3 + 3]) {
                    continue;
                }
                assert!(
                    (got as f64 - expected.as_f64().unwrap()).abs() < tolerance,
                    "{} LUT={lut} {boundary:?}: {got} vs {expected}",
                    case["name"]
                );
            }
        }
    }
}

#[test]
fn loaded_models_still_agree_with_manufacturer_source_points() {
    let report: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/bw-expansion-review/preparation-report.json"
    ))
    .unwrap();
    for (stock, origin_key, source, tolerance) in [
        (
            FILM,
            "film_source_exposure_origin",
            include_str!("../../../data/profile-sources/bw-expansion/trix400-d76-8min.csv"),
            0.01,
        ),
        (
            PAPER,
            "paper_source_exposure_origin",
            include_str!("../../../data/profile-sources/bw-expansion/ilford-mgrc-grade2.csv"),
            0.03,
        ),
    ] {
        let p = profile::load_profile_by_name(&data_dir(), stock).unwrap();
        let origin = report[origin_key].as_f64().unwrap();
        let x = &p.data.log_exposure;
        let d = &p.data.density_curves;
        // Film development removes the sampled net-density floor. Account for
        // that actual runtime convention when checking total source density.
        let floor = if stock == FILM { d[0][0] } else { 0.0 };
        for row in source.lines().skip(1) {
            let values: Vec<f64> = row.split(',').map(|v| v.parse().unwrap()).collect();
            let at = values[0] - origin;
            let i = x.partition_point(|v| *v < at).clamp(1, x.len() - 1);
            let t = ((at - x[i - 1]) / (x[i] - x[i - 1])).clamp(0.0, 1.0);
            let total = d[i - 1][0] * (1.0 - t) + d[i][0] * t - floor + p.data.base_density[0];
            assert!(
                (total - values[1]).abs() < tolerance,
                "{stock}: source {row}, runtime {total}"
            );
        }
    }
}

#[test]
fn paper_reflection_density_is_counted_once_including_base() {
    let paper = profile::resolve_for_render(
        profile::load_profile_by_name(&data_dir(), PAPER).unwrap(),
        None,
    );
    assert!(paper.is_paper() && paper.is_printing() && paper.is_bw() && paper.is_negative());
    let cd: Vec<[f64; 3]> = paper
        .data
        .channel_density
        .iter()
        .map(|r| [r[0], r[1], r[2]])
        .collect();
    let illuminant = spectral::ILLUMINANT_D50_F64;
    let norm: f64 = illuminant
        .iter()
        .zip(spectral::CMF_Y_F64)
        .map(|(a, b)| a * b)
        .sum();
    // Independent Beer-Lambert definition for already-measured reflection D:
    // R = 10^(-D). A second pass or three replicated absorbers would fail.
    for density in [0.0, 0.5, 1.0, 2.0] {
        let image = ImageBuf::from_data(1, 1, vec![from_f64(density); 3]);
        let xyz = scan_log_xyz_cpu(&image, &cd, &paper.data.base_density, &illuminant, norm);
        let expected = 10f64.powf(-(density + 7.0 / 133.5));
        assert!((10f64.powf(xyz[1]) - expected).abs() < 2e-8);
    }
}

#[test]
fn normalized_neutral_negative_printing_is_independent_of_paper_spectral_weight() {
    let film = profile::load_profile_by_name(&data_dir(), FILM).unwrap();
    assert_eq!(film.info.target_print.as_deref(), Some(PAPER));
    let original = profile::load_profile_by_name(&data_dir(), PAPER).unwrap();
    let input = ImageBuf::from_data(
        9,
        1,
        [0.0, 1e-12, 1e-6, 0.01, 0.184, 0.5, 1.0, 10.0, 1000.0]
            .into_iter()
            .flat_map(|v| [from_f64(v); 3])
            .collect(),
    );
    let baseline =
        Pipeline::new_with_spectral(film.clone(), original.clone(), params(), &data_dir())
            .unwrap()
            .process(input.clone(), &CpuBackend);
    for band in [0, 1, 2] {
        let mut paper = original.clone();
        for (i, row) in paper.data.log_sensitivity.iter_mut().enumerate() {
            // Deliberately unrelated blue, green, and all-visible weights.
            row[0] = if band == 2 || (band == 0 && i == 10) || (band == 1 && i == 30) {
                0.7
            } else {
                f64::NAN
            };
        }
        let out = Pipeline::new_with_spectral(film.clone(), paper, params(), &data_dir())
            .unwrap()
            .process(input.clone(), &CpuBackend);
        for (&a, &b) in out.data.iter().zip(&baseline.data) {
            assert!(a.is_finite() && (a as f64 - b as f64).abs() < 3e-5);
        }
    }
    for pair in baseline.data.chunks_exact(3).collect::<Vec<_>>().windows(2) {
        assert!(pair[1][1] >= pair[0][1], "gray ramp reversed");
    }
}
