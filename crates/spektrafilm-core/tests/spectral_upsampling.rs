//! Spectral upsampling and neutral print filters against frozen upstream
//! experimental 28bf883 references
//! (scripts/parity/freeze_spectral_upsampling_reference.py): the tc LUT of
//! every shipped method with its white-balance normalisation, and the filter
//! database lookup with its C0 M50 Y50 fallback.

use serde::Deserialize;
use spektrafilm_core::{params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_math::spectral;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Fixture {
    tc_luts: Vec<LutCase>,
    neutral_print_filters: Vec<FilterCase>,
}

#[derive(Deserialize)]
struct LutCase {
    stock: String,
    method: String,
    size: usize,
    indices: Vec<usize>,
    /// `[i][j][channel]` at `indices`; one channel for B&W stocks upstream.
    cells: Vec<Vec<Vec<f64>>>,
    /// The table sampled (Mitchell cubic) where a neutral input lands.
    neutral_raw: Vec<f64>,
}

#[derive(Deserialize)]
struct FilterCase {
    film: String,
    paper: String,
    illuminant: String,
    cmy: [f64; 3],
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!(
        "fixtures/spectral_upsampling_python_28bf883.json"
    ))
    .unwrap()
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn pipeline(film: &str, paper: &str, params: RuntimeParams) -> Pipeline {
    let data = data_dir();
    Pipeline::new_with_spectral(
        profile::load_profile_by_name(&data, film).unwrap(),
        profile::load_profile_by_name(&data, paper).unwrap(),
        params,
        &data,
    )
    .unwrap()
}

fn assert_close(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() <= 1e-12 * (1.0 + want.abs()),
        "{what}: {got} vs {want}"
    );
}

/// hanatos2025 balances its adaptation window on the LUT's own neutral at
/// the reference illuminant; the reflectance methods relight by the
/// reference illuminant and normalise each channel on their neutral at the
/// D65 scene white, so that neutral lands on (1, 1, 1). Built without input
/// gamut compression, which has its own fixtures.
#[test]
fn tc_luts_match_python_experimental() {
    let fixture = fixture();
    assert_eq!(fixture.tc_luts.len(), 9);
    for case in fixture.tc_luts {
        let mut params = RuntimeParams::default();
        params.settings.rgb_to_raw_method = case.method.clone();
        params.io.input_gamut_compress.algorithm = "off".into();
        let pipe = pipeline(&case.stock, "kodak_portra_endura", params);
        let lut = pipe.tc_lut().expect("every method builds a tc LUT");
        assert_eq!((lut.size, lut.channels), (case.size, 3));
        let what = format!("{} {}", case.stock, case.method);
        for (a, &i) in case.indices.iter().enumerate() {
            for (b, &j) in case.indices.iter().enumerate() {
                let want = &case.cells[a][b];
                let got = &lut.data[(i * lut.size + j) * 3..][..3];
                for c in 0..3 {
                    // B&W: one upstream channel, three equal lanes here.
                    let w = want[c.min(want.len() - 1)];
                    assert_close(got[c], w, &format!("{what} cell ({i}, {j}) ch {c}"));
                }
            }
        }

        let white = match case.method.as_str() {
            "hanatos2025" => &spectral::ILLUMINANT_D55, // every stock here is D55-balanced
            _ => &spectral::ILLUMINANT_D65,
        };
        let (x, y) = spectral::illuminant_to_xy(white);
        let (tx, ty) = spectral::xy_to_tc(x, y);
        let scale = (lut.size - 1) as f64;
        let neutral = spektrafilm_math::lut::bicubic_2d_f64(
            &lut.data,
            lut.size,
            lut.size,
            3,
            tx * scale,
            ty * scale,
        );
        for c in 0..3 {
            let w = case.neutral_raw[c.min(case.neutral_raw.len() - 1)];
            assert_close(neutral[c], w, &format!("{what} neutral ch {c}"));
            if case.method != "hanatos2025" {
                assert!(
                    (neutral[c] - 1.0).abs() < 1e-12,
                    "{what}: neutral {neutral:?}"
                );
            }
        }
    }
}

/// Database entries apply as shipped; combinations it lacks, such as the
/// Negs-only Tri-X 400 D-76 and Ilford MGRC stocks, print with C0 M50 Y50.
#[test]
fn neutral_print_filters_match_python_experimental() {
    for case in fixture().neutral_print_filters {
        let mut params = RuntimeParams::default();
        params.settings.neutral_print_filters_from_database = true;
        assert_eq!(params.enlarger.illuminant, case.illuminant);
        let pipe = pipeline(&case.film, &case.paper, params);
        let e = &pipe.params.enlarger;
        let got = [e.c_filter_neutral, e.m_filter_neutral, e.y_filter_neutral];
        let want = case.cmy.map(|v| v as f32);
        assert_eq!(got, want, "{} -> {}", case.film, case.paper);
    }
}

/// The reflectance methods run through the GPU-resident chain, whose front
/// pass projects under their D65 scene white, and match the CPU per-stage
/// path within the established f32 resident-chain budget. Midtone inputs
/// only: output highlight clipping is the known outlier regime.
#[test]
fn reflectance_methods_gpu_resident_matches_cpu() {
    use spektrafilm_math::{image::ImageBuf, precision::from_f64};
    let Some(gpu) = spektrafilm_gpu::wgpu_backend::WgpuBackend::new() else {
        eprintln!("no GPU adapter available; skipping");
        return;
    };
    for method in ["arctic2026beta04", "mallett2019"] {
        let mut params = RuntimeParams::default();
        params.settings.rgb_to_raw_method = method.into();
        params.camera.auto_exposure = false;
        // Nonzero EV so the resident path's exposure fold is exercised.
        params.camera.exposure_compensation_ev = 0.5;
        params.film_render.grain.active = false;
        // Output gamut compression forces the per-stage path.
        params.io.output_gamut_compress.algorithm = "off".into();
        let pipe = pipeline("kodak_portra_400", "kodak_portra_endura", params);

        let (w, h) = (16u32, 16u32);
        let data: Vec<_> = (0..w * h * 3)
            .map(|i| from_f64(0.05 + 0.5 * ((i * 37) % 256) as f64 / 255.0))
            .collect();
        let img = ImageBuf::from_data(w, h, data);
        let out_gpu = pipe.process(img.clone(), &gpu);
        let out_cpu = pipe.process(img, &spektrafilm_gpu::cpu_backend::CpuBackend);
        assert!(
            out_cpu.data.iter().all(|v| (*v as f64).is_finite()),
            "{method}"
        );
        let diffs: Vec<f64> = out_gpu
            .data
            .iter()
            .zip(&out_cpu.data)
            .map(|(g, c)| (*g as f64 - *c as f64).abs())
            .collect();
        let mean = diffs.iter().sum::<f64>() / diffs.len() as f64;
        let max = diffs.iter().cloned().fold(0.0, f64::max);
        assert!(
            mean < 5e-3 && max < 5e-2,
            "{method}: GPU vs CPU mean {mean:.2e} max {max:.2e}"
        );
    }
}

/// An unknown upsampler method is rejected at calibration, as upstream.
#[test]
fn unknown_rgb_to_raw_method_errors() {
    let data = data_dir();
    let mut params = RuntimeParams::default();
    params.settings.rgb_to_raw_method = "bogus".into();
    let result = Pipeline::new_with_spectral(
        profile::load_profile_by_name(&data, "kodak_portra_400").unwrap(),
        profile::load_profile_by_name(&data, "kodak_portra_endura").unwrap(),
        params,
        &data,
    );
    match result {
        Err(e) => assert!(e.contains("unsupported rgb_to_raw_method"), "{e}"),
        Ok(_) => panic!("expected an error for an unknown method"),
    }
}
