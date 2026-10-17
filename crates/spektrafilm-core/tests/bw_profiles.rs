//! Frozen Python experimental 28bf883 references for the updated B&W stocks.
//! Gamut compression is disabled to isolate the stock/calibration port from
//! the intentionally different colour-management defaults in the two repos.

use serde::Deserialize;
use spektrafilm_core::{params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_gpu::cpu_backend::CpuBackend;
use spektrafilm_math::{image::ImageBuf, precision::from_f32};
use std::path::Path;

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    film: String,
    paper: String,
    scan_film: bool,
    film_development_time: Option<f64>,
    print_development_time: Option<f64>,
    expected_rgb: Vec<f64>,
}

#[test]
fn bw_stocks_match_python_with_and_without_luts() {
    let fixtures: Fixture =
        serde_json::from_str(include_str!("fixtures/bw_python_28bf883.json")).unwrap();
    let data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    for case in fixtures.cases {
        for lut in [false, true] {
            let mut params = RuntimeParams::default();
            params.camera.auto_exposure = false;
            params.camera.lens_blur_um = 0.0;
            params.enlarger.lens_blur = 0.0;
            params.film_render.grain.active = false;
            params.film_render.halation.active = false;
            params.film_render.dir_couplers.active = false;
            params.film_render.glare.active = false;
            params.print_render.glare.active = false;
            params.film_render.development_time = case.film_development_time;
            params.print_render.development_time = case.print_development_time;
            params.scanner.lens_blur = 0.0;
            params.scanner.unsharp_mask = [0.0, 0.0];
            params.io.input_color_space = "sRGB".into();
            params.io.input_cctf_decoding = false;
            params.io.output_color_space = "sRGB".into();
            params.io.output_cctf_encoding = false;
            params.io.input_gamut_compress.algorithm = "off".into();
            params.io.output_gamut_compress.algorithm = "off".into();
            params.io.scan_film = case.scan_film;
            params.settings.use_enlarger_lut = lut;
            params.settings.use_scanner_lut = lut;
            let film = profile::load_profile_by_name(&data_dir, &case.film).unwrap();
            let paper = if case.scan_film {
                &case.film
            } else {
                &case.paper
            };
            let print = profile::load_profile_by_name(&data_dir, paper).unwrap();
            let pipeline = Pipeline::new_with_spectral(film, print, params, &data_dir).unwrap();
            let input = ImageBuf::from_data(
                4,
                4,
                (0..48)
                    .map(|i| from_f32((0.05 + 0.5 * ((i * 37) % 256) as f64 / 255.0) as f32))
                    .collect(),
            );
            let output = pipeline.process(input, &CpuBackend);
            assert_eq!(output.data.len(), case.expected_rgb.len());
            #[cfg(feature = "precision-f64")]
            let tolerance = 1e-6;
            #[cfg(not(feature = "precision-f64"))]
            let tolerance = 3e-3;
            for (i, (&got, &expected)) in output.data.iter().zip(&case.expected_rgb).enumerate() {
                assert!(
                    got.is_finite() && (got as f64 - expected).abs() < tolerance,
                    "{} -> {paper}, film dev {:?}, print dev {:?}, LUT={lut}, component {i}: {got} vs {expected}",
                    case.film,
                    case.film_development_time,
                    case.print_development_time
                );
            }
        }
    }
}
