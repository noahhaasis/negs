mod debug_compare;
pub mod filming;
pub mod printing;
pub mod scanning;

#[cfg(test)]
mod integration_tests {
    use crate::params::RuntimeParams;
    use crate::pipeline::Pipeline;
    use crate::profile;
    use spektrafilm_math::image::ImageBuf;
    use spektrafilm_math::precision::{Scalar, from_f64};
    use std::path::Path;

    fn data_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("data")
    }

    #[test]
    fn test_full_pipeline_portra_400_to_endura() {
        let dir = data_dir();
        let film = profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap();
        let print = profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap();

        let mut params = RuntimeParams::default();
        params.film_render.grain.active = false;
        params.film_render.halation.active = false;
        params.film_render.dir_couplers.active = false;
        params.camera.auto_exposure = false;

        let backend = spektrafilm_gpu::cpu_backend::CpuBackend;
        let img = ImageBuf::from_data(8, 8, vec![from_f64(0.184); 8 * 8 * 3]);

        let pipeline = Pipeline::new(film, print, params);
        let result = pipeline.process(img, &backend);

        assert_eq!(result.width, 8);
        assert_eq!(result.height, 8);
        let px = result.get(4, 4);
        for c in 0..3 {
            assert!(
                px[c] >= from_f64(0.0) && px[c] <= from_f64(1.0),
                "channel {c} out of range: {}",
                px[c]
            );
        }
        let mean: Scalar =
            result.data.iter().copied().sum::<Scalar>() / result.data.len() as Scalar;
        assert!(mean > from_f64(0.01), "output near-black: mean={mean}");
        assert!(mean < from_f64(0.99), "output near-white: mean={mean}");
    }

    #[test]
    fn test_full_pipeline_with_spectral_lut() {
        let dir = data_dir();
        let film = profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap();
        let print = profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap();

        let mut params = RuntimeParams::default();
        params.film_render.grain.active = false;
        params.film_render.halation.active = false;
        params.film_render.dir_couplers.active = false;
        params.camera.auto_exposure = false;
        params.io.input_color_space = "sRGB".to_string();

        let backend = spektrafilm_gpu::cpu_backend::CpuBackend;
        let img = ImageBuf::from_data(8, 8, vec![from_f64(0.184); 8 * 8 * 3]);

        let pipeline = Pipeline::new_with_spectral(film, print, params, &dir);
        match pipeline {
            Ok(p) => {
                let result = p.process(img, &backend);
                let mean: Scalar =
                    result.data.iter().copied().sum::<Scalar>() / result.data.len() as Scalar;
                eprintln!("Spectral pipeline output mean: {mean}");
                let px = result.get(4, 4);
                eprintln!("Spectral pipeline pixel(4,4): {:?}", px);
                assert!(
                    mean > from_f64(0.01),
                    "spectral output near-black: mean={mean}"
                );
                assert!(
                    mean < from_f64(0.99),
                    "spectral output near-white: mean={mean}"
                );
            }
            Err(e) => {
                eprintln!("Spectral LUT not available: {e} — skipping test");
            }
        }
    }

    /// Full default chain vs upstream experimental 28bf883 (CAT16, input
    /// gamut `xy`, output gamut `cam16ucs`, model-derived curves). Reference
    /// values from `scripts/parity/exp_data_unit_references.py`: 28bf883's
    /// pipeline with the stochastic / spatial effects off, linear sRGB in/out,
    /// and the ticket-16 items this branch doesn't port (gamut defaults and
    /// centre) kept at Negs' current behaviour.
    #[test]
    fn full_chain_matches_python_experimental() {
        let dir = data_dir();
        let film = profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap();
        let print = profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap();

        let mut params = RuntimeParams::default();
        params.camera.auto_exposure = false;
        params.io.input_color_space = "sRGB".to_string();
        params.io.input_cctf_decoding = false;
        params.io.output_cctf_encoding = false;
        params.film_render.grain.active = false;
        params.film_render.halation.active = false;
        params.film_render.dir_couplers.active = false;
        params.print_render.glare.active = false;
        params.scanner.unsharp_mask = [0.0, 0.0];

        let backend = spektrafilm_gpu::cpu_backend::CpuBackend;
        let pipeline =
            Pipeline::new_with_spectral(film, print, params, &dir).expect("spectral LUT");

        // The f32 Scalar chain carries ~1e-4 of accumulated rounding against
        // the f64 Python reference; the f64 build tracks it tightly.
        #[cfg(feature = "precision-f64")]
        const TOL: f64 = 1e-6;
        #[cfg(not(feature = "precision-f64"))]
        const TOL: f64 = 3e-3;

        // 1×1 midgray.
        let img = ImageBuf::from_data(1, 1, vec![from_f64(0.184); 3]);
        let out = pipeline.process(img, &backend);
        let want = [
            0.18168434872578296,
            0.18165109490231654,
            0.1815334493358714,
        ];
        for c in 0..3 {
            let got = out.get(0, 0)[c] as f64;
            assert!(
                (got - want[c]).abs() < TOL,
                "midgray ch {c}: {got} vs {} (diff {:.3e})",
                want[c],
                (got - want[c]).abs()
            );
        }

        // 4×4 deterministic gradient (same integer formula on both sides).
        let (w, h) = (4u32, 4u32);
        let data: Vec<_> = (0..w * h * 3)
            .map(|i| from_f64(0.05 + 0.5 * ((i * 37) % 256) as f64 / 255.0))
            .collect();
        let img = ImageBuf::from_data(w, h, data);
        let out = pipeline.process(img, &backend);
        #[rustfmt::skip]
        let want: [[f64; 3]; 16] = [
            [0.032376316770347, 0.10652646692566108, 0.15814299888279826],
            [0.29524463655843336, 0.3694247495808824, 0.4237204166054831],
            [0.3468431375595474, 0.07822039077665163, 0.13024133284612743],
            [0.22099123444655544, 0.2988841002297578, 0.356874734618922],
            [0.4201983960013168, 0.34710778612981563, 0.09304640446390064],
            [0.13886429145461016, 0.21868548143177954, 0.2781940047075912],
            [0.37743216976827965, 0.4444592967632268, 0.4920634353703614],
            [0.052509888765679066, 0.13063132998644708, 0.18591466770817738],
            [0.3135798769252942, 0.38642253672376825, 0.4392258829687226],
            [0.37022906818202134, 0.10061806317075594, 0.15391198191629965],
            [0.24159486114934672, 0.31845742397189414, 0.3757881694847447],
            [0.4360168431769521, 0.3688569348738102, 0.11572887793528597],
            [0.16132265109775312, 0.2409203704547354, 0.3002687711937394],
            [0.3923247782689134, 0.45789103453716296, 0.5036896147201078],
            [0.07481942428620926, 0.1545457388929201, 0.21196056140545308],
            [0.3312832787229991, 0.4027565580913274, 0.4545184889118128],
        ];
        let mut max_diff = 0.0f64;
        for (i, want_px) in want.iter().enumerate() {
            let px = out.get(i as u32 % w, i as u32 / w);
            for c in 0..3 {
                let d = (px[c] as f64 - want_px[c]).abs();
                max_diff = max_diff.max(d);
                assert!(
                    d < TOL,
                    "px {i} ch {c}: {} vs {} (diff {d:.3e})",
                    px[c],
                    want_px[c]
                );
            }
        }
        eprintln!("full chain: max diff vs Python experimental {max_diff:.3e}");
    }

    #[test]
    fn test_film_scan_pipeline() {
        let dir = data_dir();
        let film = profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap();
        let print = film.clone();

        let mut params = RuntimeParams::default();
        params.io.scan_film = true;
        params.film_render.grain.active = false;
        params.film_render.halation.active = false;
        params.film_render.dir_couplers.active = false;
        params.camera.auto_exposure = false;

        let backend = spektrafilm_gpu::cpu_backend::CpuBackend;
        let img = ImageBuf::from_data(4, 4, vec![from_f64(0.184); 4 * 4 * 3]);

        let pipeline = Pipeline::new(film, print, params);
        let result = pipeline.process(img, &backend);
        let mean: Scalar =
            result.data.iter().copied().sum::<Scalar>() / result.data.len() as Scalar;
        assert!(mean > from_f64(0.01), "film scan near-black: mean={mean}");
        assert!(mean < from_f64(0.99), "film scan near-white: mean={mean}");
    }

    #[test]
    fn with_params_recalibrates_print_exposure_like_a_new_pipeline() {
        let dir = data_dir();
        let film = profile::load_profile_by_name(&dir, "kodak_gold_200").unwrap();
        let print = profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap();
        let backend = spektrafilm_gpu::cpu_backend::CpuBackend;
        let img = ImageBuf::from_data(
            8,
            8,
            (0..8 * 8 * 3)
                .map(|i| from_f64((i % 17) as f64 / 16.0))
                .collect(),
        );
        let cached = Pipeline::new_with_spectral(
            film.clone(),
            print.clone(),
            RuntimeParams::default(),
            &dir,
        )
        .unwrap();
        for (ev, compensation, gamma) in [(1.3, true, 1.0), (-0.7, true, 1.2), (1.3, false, 1.0)] {
            let mut params = RuntimeParams::default();
            params.camera.exposure_compensation_ev = ev;
            params.enlarger.print_exposure_compensation = compensation;
            params.film_render.density_curves_morph.active = gamma != 1.0;
            params.film_render.density_curves_morph.gamma_factor = gamma;
            let fresh =
                Pipeline::new_with_spectral(film.clone(), print.clone(), params.clone(), &dir)
                    .unwrap();
            let reused = cached.clone().with_params(params);
            assert_eq!(
                reused.print_exposure_factor().to_bits(),
                fresh.print_exposure_factor().to_bits(),
                "EV {ev}, compensation {compensation}, gamma {gamma}"
            );
            assert_eq!(
                reused.process(img.clone(), &backend).data,
                fresh.process(img.clone(), &backend).data
            );
        }
    }

    #[test]
    fn positive_slide_film_scan_stays_photographic() {
        let dir = data_dir();
        let film = profile::load_profile_by_name(&dir, "kodak_ektachrome_100").unwrap();
        let print = film.clone();

        let mut params = RuntimeParams::default();
        params.io.scan_film = true;
        params.camera.auto_exposure = false;

        let backend = spektrafilm_gpu::cpu_backend::CpuBackend;
        let mut data = Vec::with_capacity(64 * 64 * 3);
        for y in 0..64 {
            for x in 0..64 {
                data.push(from_f64(x as f64 / 63.0));
                data.push(from_f64(y as f64 / 63.0));
                data.push(from_f64(((x + y) as f64 / 126.0).powf(1.2)));
            }
        }
        let img = ImageBuf::from_data(64, 64, data);

        let pipeline = Pipeline::new_with_spectral(film, print, params.clone(), &dir)
            .unwrap()
            .with_params(params);
        assert_eq!(
            pipeline.params.film_render.dir_couplers.gamma_samelayer_rgb,
            [0.2291, 0.1029, 0.2651],
            "GUI-style with_params must preserve positive-film DIR parameters"
        );
        let result = pipeline.process(img, &backend);
        let mut clipped_low = 0usize;
        let mut clipped_high = 0usize;
        for c in 0..3 {
            let values = result.data.iter().skip(c).step_by(3);
            let (mut min, mut max, mut sum) = (Scalar::INFINITY, Scalar::NEG_INFINITY, 0.0);
            let mut n = 0usize;
            for &v in values {
                min = min.min(v);
                max = max.max(v);
                sum += v;
                if v <= from_f64(0.0) {
                    clipped_low += 1;
                }
                if v >= from_f64(1.0) {
                    clipped_high += 1;
                }
                n += 1;
            }
            let mean = sum / n as Scalar;
            assert!(
                mean > from_f64(0.2) && mean < from_f64(0.8),
                "slide scan channel {c} mean is implausible: {mean} (min={min}, max={max})"
            );
        }
        let total = result.data.len();
        assert!(
            clipped_low < total / 20,
            "slide scan has too many black-clipped samples: {clipped_low}/{total}"
        );
        assert!(
            clipped_high < total / 20,
            "slide scan has too many white-clipped samples: {clipped_high}/{total}"
        );
    }
}

#[cfg(test)]
mod debug_tests {
    use crate::params::RuntimeParams;
    use crate::profile;
    use crate::stages;
    use spektrafilm_math::image::ImageBuf;
    use spektrafilm_math::precision::from_f64;
    use std::path::Path;

    #[test]
    fn debug_pipeline_values() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("data");
        let film = profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap();
        let print = profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap();

        let mut params = RuntimeParams::default();
        params.film_render.grain.active = false;
        params.film_render.halation.active = false;
        params.film_render.dir_couplers.active = false;
        params.camera.auto_exposure = false;
        params.io.input_color_space = "sRGB".to_string();

        let backend = spektrafilm_gpu::cpu_backend::CpuBackend;
        let gray = from_f64(0.184);
        let img = ImageBuf::from_data(1, 1, vec![gray, gray, gray]);
        eprintln!("Input: {:?}", img.get(0, 0));

        let ref_illuminant = crate::illuminants::select_illuminant(&film.info.reference_illuminant);
        let log_raw = stages::filming::expose(
            img,
            &film,
            &params,
            &backend,
            None,
            &ref_illuminant,
            1.0,
        );
        eprintln!("log_raw: {:?}", log_raw.get(0, 0));

        let density_cmy = stages::filming::develop(log_raw, &film, &params, &backend);
        eprintln!("density_cmy: {:?}", density_cmy.get(0, 0));

        // Use simplified printing path for debug trace
        let printed = stages::printing::process(&density_cmy, &film, &print, &params, &backend);
        eprintln!("density_print: {:?}", printed.get(0, 0));
        let density_print = printed;
        let rgb_out = stages::scanning::scan(
            density_print,
            &print,
            &params,
            &backend,
            &crate::color_reference::ColorReference::identity(),
            &crate::gamut_compression::OutputGamutCompress::identity(),
        );
        eprintln!("rgb_out: {:?}", rgb_out.get(0, 0));
    }
}
