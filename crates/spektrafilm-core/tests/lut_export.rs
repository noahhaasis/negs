use std::path::Path;

use spektrafilm_core::{lut_export, params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_gpu::cpu_backend::CpuBackend;
use spektrafilm_math::{image::ImageBuf, precision::from_f64};

fn rows(cube: &str) -> Vec<[f64; 3]> {
    cube.lines()
        .filter(|s| s.starts_with(|c: char| c.is_ascii_digit() || c == '-'))
        .map(|s| {
            let values: Vec<f64> = s.split_whitespace().map(|v| v.parse().unwrap()).collect();
            assert_eq!(values.len(), 3);
            assert!(
                values
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            );
            [values[0], values[1], values[2]]
        })
        .collect()
}

#[test]
fn exported_cube_matches_spectral_render_of_independent_pixels() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    for (film, paper, scan) in [
        ("kodak_gold_200", "kodak_portra_endura", false),
        ("kodak_doublex", "kodak_2302", false),
        ("kodak_trix", "unused_paper", true),
    ] {
        let mut params = RuntimeParams::default();
        params.camera.exposure_compensation_ev = 0.7;
        params.film_render.density_curves_morph.active = true;
        params.film_render.density_curves_morph.gamma_factor = 1.1;
        params.film_render.development_time = Some(9.0);
        params.print_render.development_time = Some(4.0);
        params.enlarger.y_filter_shift = 8.0;
        params.enlarger.m_filter_shift = -5.0;
        params.enlarger.print_exposure = 1.2;
        params.io.scan_film = scan;
        // These must not resize, contaminate, or finish the LUT's grid.
        params.io.upscale_factor = 0.25;
        params.camera.diffusion_filter.active = true;
        params.enlarger.diffusion_filter.active = true;
        params.camera.lens_blur_um = 40.0;
        params.scanner.lens_blur = 2.0;
        params.film_render.halation.boost_ev = 2.0;
        params.post.brightness = 50.0;
        let text = lut_export::cube(&data, film, paper, params.clone(), 33).unwrap();
        assert!(text.contains("LUT_3D_SIZE 33\n"));
        assert!(text.contains("DOMAIN_MIN 0.0 0.0 0.0\nDOMAIN_MAX 1.0 1.0 1.0"));
        let values = rows(&text);
        assert_eq!(values.len(), 33_usize.pow(3));
        params.enlarger.preflash_exposure = 0.02;
        params.scanner.white_correction = true;
        params.settings.use_enlarger_lut = true;
        params.settings.use_scanner_lut = true;
        let clean = lut_export::per_pixel_params(params.clone());
        // Python's lut_mode bakes the per-image trims neutral and computes
        // exactly; manual exposure stays (see per_pixel_params).
        assert_eq!(
            (clean.enlarger.y_filter_shift, clean.enlarger.m_filter_shift),
            (0.0, 0.0)
        );
        assert_eq!(clean.enlarger.preflash_exposure, 0.0);
        assert!(!clean.scanner.white_correction);
        assert!(!clean.settings.use_enlarger_lut && !clean.settings.use_scanner_lut);
        assert_eq!(clean.camera.exposure_compensation_ev, 0.7);
        assert_eq!(clean.enlarger.print_exposure, 1.2);
        let pipeline = Pipeline::new_with_spectral(
            profile::load_profile_by_name(&data, film).unwrap(),
            profile::load_profile_by_name(&data, if scan { film } else { paper }).unwrap(),
            clean.clone(),
            &data,
        )
        .unwrap();
        // Re-render isolated pixels: catches incorrect CUBE ordering and any
        // spatial operation leaking between adjacent grid entries.
        for [r, g, b] in [
            [0, 0, 0],
            [32, 32, 32],
            [8, 16, 24],
            [31, 1, 12],
            [12, 12, 12],
        ] {
            let input = ImageBuf::from_data(
                1,
                1,
                [r, g, b]
                    .map(|v| spektrafilm_math::precision::srgb_decode(from_f64(v as f64 / 32.0)))
                    .to_vec(),
            );
            let expected = pipeline.process(input, &CpuBackend);
            let got = values[b * 33 * 33 + g * 33 + r];
            for c in 0..3 {
                assert!(
                    (got[c] - expected.data[c] as f64).abs() < 2e-6,
                    "{film}, [{r}, {g}, {b}], channel {c}: {} vs {}",
                    got[c],
                    expected.data[c]
                );
            }
        }
        // Image Size never enlarges the grid or changes its entries.
        let mut sized = params.clone();
        sized.io.upscale_factor = 1.0;
        sized.io.photo_long_side_px = Some(12_000);
        assert_eq!(lut_export::cube(&data, film, paper, sized, 33).unwrap(), text);
        // Different manual settings must produce a different LUT.
        let mut changed = clean.clone();
        changed.camera.exposure_compensation_ev = -1.0;
        assert_ne!(
            rows(&lut_export::cube(&data, film, paper, changed, 3).unwrap()),
            rows(&lut_export::cube(&data, film, paper, clean, 3).unwrap())
        );
    }
}

/// A preset's explicit DIR gammas reach the LUT; the stock's own gammas,
/// set explicitly, give the same LUT as leaving them unset.
#[test]
fn explicit_dir_gammas_reach_the_cube() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let (film, paper) = ("kodak_portra_400", "kodak_portra_endura");
    let unset = lut_export::cube(&data, film, paper, RuntimeParams::default(), 5).unwrap();

    let mut stock = RuntimeParams::default();
    stock.film_render.dir_couplers.gamma_samelayer_rgb = Some([0.5159, 0.5934, 0.2829]);
    assert_eq!(lut_export::cube(&data, film, paper, stock, 5).unwrap(), unset);

    let mut cooked = RuntimeParams::default();
    cooked.film_render.dir_couplers.gamma_samelayer_rgb = Some([0.52, 0.51, 0.26]);
    cooked.film_render.dir_couplers.gamma_interlayer_r_to_gb = Some([0.16, 0.2]);
    assert_ne!(rows(&lut_export::cube(&data, film, paper, cooked, 5).unwrap()), rows(&unset));
}
