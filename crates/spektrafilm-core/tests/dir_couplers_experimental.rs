//! DIR couplers against frozen Python experimental 28bf883 values (ticket
//! 16.6): the `couplers.toml` presets per stock, and at unit level the
//! Langmuir terms, the curve inversion and the spatial correction. Unit level
//! because the profiles and model curves are ported separately; the curves
//! here are the fixture's (sampled from 28bf883's fitted models).
//!
//! Regenerate the fixture with
//! `scripts/parity/freeze_dir_couplers_reference.py` (Python only, never from
//! Rust output).

use serde::Deserialize;
use spektrafilm_core::{
    params::{DirCouplersParams, RuntimeParams},
    pipeline::Pipeline,
    profile,
};
use spektrafilm_gpu::{ComputeBackend, cpu_backend::CpuBackend};
use spektrafilm_math::{
    image::ImageBuf,
    interp::np_interp,
    precision::{from_f64, to_f64},
};
use spektrafilm_model::couplers::{
    DirCouplersModel, Langmuir, apply_density_correction, compute_dir_couplers_matrix,
    langmuir_donor_params, receiver_langmuir_params,
};
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize)]
struct Fixture {
    np_interp: NpInterp,
    films: HashMap<String, FilmCurves>,
    model_cases: Vec<ModelCase>,
    presets: Vec<Preset>,
}

#[derive(Deserialize)]
struct NpInterp {
    x: Vec<f64>,
    xp: Vec<f64>,
    fp: Vec<f64>,
    expected: Vec<f64>,
}

#[derive(Deserialize)]
struct FilmCurves {
    log_exposure: Vec<f64>,
    /// Normalized, `[n][n_ch]`.
    density_curves: Vec<Vec<f64>>,
}

/// JSON null is infinity (a linear Langmuir channel).
type MaybeInf = Option<f64>;

#[derive(Deserialize)]
struct ModelCase {
    name: String,
    film: String,
    #[serde(rename = "type")]
    film_type: String,
    n_ch: usize,
    dir_couplers: DirCouplersParams,
    matrix_unit: Vec<Vec<f64>>,
    langmuir_k: Vec<MaybeInf>,
    langmuir_d_ref: Vec<f64>,
    receiver_kr: Vec<MaybeInf>,
    receiver_c_ref: Vec<f64>,
    density_curves_0: Vec<Vec<f64>>,
    width: u32,
    height: u32,
    pixel_size_um: f64,
    log_raw: Vec<f64>,
    expected_density: Vec<f64>,
}

#[derive(Deserialize)]
struct Preset {
    film: String,
    #[serde(rename = "use")]
    usage: String,
    channel_model: String,
    reference_illuminant: String,
    dir_couplers: DirCouplersParams,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/dir_couplers_python_28bf883.json")).unwrap()
}

fn inf(v: MaybeInf) -> f64 {
    v.unwrap_or(f64::INFINITY)
}

fn close(got: f64, want: f64, tolerance: f64) -> bool {
    if want.is_infinite() {
        return got == want;
    }
    (got - want).abs() <= tolerance * want.abs().max(1.0)
}

/// What `apply_film_specific_params` does to a B&W stock: the 3-channel
/// engine runs upstream's single channel on all three.
fn broadcast_bw(dir: &mut DirCouplersParams) {
    dir.gamma_samelayer_rgb = Some([dir.gamma_samelayer()[0]; 3]);
    dir.gamma_interlayer_r_to_gb = Some([0.0; 2]);
    dir.gamma_interlayer_g_to_rb = Some([0.0; 2]);
    dir.gamma_interlayer_b_to_rg = Some([0.0; 2]);
    dir.langmuir_donor_k_rgb = [dir.langmuir_donor_k_rgb[0]; 3];
    dir.langmuir_receiver_k_rgb = [dir.langmuir_receiver_k_rgb[0]; 3];
}

/// `[n][n_ch]` rows as 3-channel rows (B&W: channel 0 on all three).
fn rows3(rows: &[Vec<f64>]) -> Vec<[f64; 3]> {
    rows.iter()
        .map(|r| std::array::from_fn(|c| r[c.min(r.len() - 1)]))
        .collect()
}

fn check_langmuir(case: &ModelCase, what: &str, got: &Langmuir, k: &[MaybeInf], x_ref: &[f64]) {
    for c in 0..3 {
        let p = c.min(case.n_ch - 1);
        assert!(
            close(got.k[c], inf(k[p]), 1e-12) && close(got.x_ref[c], x_ref[p], 1e-12),
            "{}: {what} channel {c}: {:?} vs k {:?} ref {:?}",
            case.name,
            got,
            k,
            x_ref
        );
    }
}

#[test]
fn np_interp_matches_numpy_on_a_folded_axis() {
    let f = fixture().np_interp;
    assert!(
        f.xp.windows(2).any(|w| w[1] < w[0]),
        "the axis must fold back"
    );
    let got = np_interp(&f.x, &f.xp, &f.fp);
    for (i, (g, w)) in got.iter().zip(&f.expected).enumerate() {
        assert!((g - w).abs() <= 1e-12, "x[{i}]: {g} vs {w}");
    }
}

#[test]
fn dir_couplers_model_matches_python() {
    let fixture = fixture();
    assert_eq!(fixture.model_cases.len(), 12);
    #[cfg(feature = "precision-f64")]
    let tolerance = 1e-6;
    #[cfg(not(feature = "precision-f64"))]
    let tolerance = 2e-3;
    let mut worst: Vec<(String, f64, f64)> = Vec::new();
    for case in &fixture.model_cases {
        let film = &fixture.films[&case.film];
        let curves = rows3(&film.density_curves);
        let positive = case.film_type == "positive";
        let mut dir = case.dir_couplers.clone();
        if case.n_ch == 1 {
            broadcast_bw(&mut dir);
        }

        let matrix = compute_dir_couplers_matrix(
            dir.gamma_samelayer(),
            dir.gamma_r_to_gb(),
            dir.gamma_g_to_rb(),
            dir.gamma_b_to_rg(),
            dir.inhibition_samelayer,
            dir.inhibition_interlayer,
        );
        for k in 0..case.n_ch {
            for m in 0..case.n_ch {
                assert!(
                    close(matrix[k][m], case.matrix_unit[k][m], 1e-12),
                    "{}: matrix",
                    case.name
                );
            }
        }
        let donor = langmuir_donor_params(&curves, dir.langmuir_donor_k_rgb);
        check_langmuir(
            case,
            "donor",
            &donor,
            &case.langmuir_k,
            &case.langmuir_d_ref,
        );
        let receiver = receiver_langmuir_params(donor.x_ref, &matrix, dir.langmuir_receiver_k_rgb);
        check_langmuir(
            case,
            "receiver",
            &receiver,
            &case.receiver_kr,
            &case.receiver_c_ref,
        );

        let model = DirCouplersModel::new(
            &curves,
            &film.log_exposure,
            &matrix,
            dir.amount,
            dir.langmuir_donor_k_rgb,
            dir.langmuir_receiver_k_rgb,
            positive,
        );
        assert_eq!(model.donor.is_some(), !positive, "{}", case.name);
        assert_eq!(model.receiver.is_some(), positive, "{}", case.name);
        let want0 = rows3(&case.density_curves_0);
        for (j, (got, want)) in model.density_curves_0.iter().zip(&want0).enumerate() {
            for c in 0..3 {
                assert!(
                    (got[c] - want[c]).abs() <= 1e-9,
                    "{}: density_curves_0[{j}][{c}]: {} vs {}",
                    case.name,
                    got[c],
                    want[c]
                );
            }
        }

        let n_px = (case.width * case.height) as usize;
        let log_raw: Vec<_> = (0..n_px)
            .flat_map(|i| (0..3).map(move |c| (i, c.min(case.n_ch - 1))))
            .map(|(i, c)| from_f64(case.log_raw[i * case.n_ch + c]))
            .collect();
        let log_raw = ImageBuf::from_data(case.width, case.height, log_raw);
        let density_cmy =
            CpuBackend.density_curve_interp(&log_raw, &film.log_exposure, &curves, 1.0);
        let out = apply_density_correction(
            density_cmy,
            log_raw,
            case.pixel_size_um as f32,
            &film.log_exposure,
            &model,
            dir.diffusion_size_um,
            dir.diffusion_tail_um,
            dir.diffusion_tail_weight,
            1.0,
            &CpuBackend,
        );
        // The Gaussian FIR blurs (σ < 3 px) clamp at the image edge, where
        // Python's `_fir_2d_fused` reflects. That's shared by every FIR blur
        // and the GPU, not part of the DIR port, so the band within the FIR
        // radius of the edge is left out (the no-diffusion case covers it).
        let sigma = dir.diffusion_size_um / case.pixel_size_um;
        let margin = if sigma > 0.0 && sigma < 3.0 {
            (3.0 * sigma + 0.5) as u32
        } else {
            0
        };
        assert!(
            case.width > 4 * margin && case.height > 4 * margin,
            "{}",
            case.name
        );
        let (mut max_err, mut border_err) = (0.0f64, 0.0f64);
        for y in 0..case.height {
            for x in 0..case.width {
                let i = (y * case.width + x) as usize;
                let interior = x >= margin
                    && y >= margin
                    && x + margin < case.width
                    && y + margin < case.height;
                for c in 0..3 {
                    let got = to_f64(out.data[i * 3 + c]);
                    let want = case.expected_density[i * case.n_ch + c.min(case.n_ch - 1)];
                    assert!(got.is_finite(), "{}: pixel {i} channel {c}", case.name);
                    let err = (got - want).abs();
                    if interior {
                        max_err = max_err.max(err);
                    } else {
                        border_err = border_err.max(err);
                    }
                }
            }
        }
        worst.push((case.name.clone(), max_err, border_err));
    }
    eprintln!("max |density - python| per case (interior, edge band): {worst:?}");
    for (name, err, _) in &worst {
        assert!(*err < tolerance, "{name}: max |density - python| {err:e}");
    }
}

#[test]
fn couplers_presets_match_python_for_every_film_stock() {
    let fixture = fixture();
    let data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    // The fixture must cover every branch of `_apply_couplers_preset`.
    for (usage, channel, illuminant_t) in [
        ("cine", "color", true),
        ("cine", "color", false),
        ("still", "color", false),
        ("cine", "bw", false),
    ] {
        assert!(
            fixture.presets.iter().any(|p| p.usage == usage
                && p.channel_model == channel
                && p.reference_illuminant.starts_with('T') == illuminant_t),
            "no {usage} {channel} stock (tungsten: {illuminant_t})"
        );
    }
    for preset in &fixture.presets {
        let film = profile::load_profile_by_name(&data_dir, &preset.film).unwrap();
        assert_eq!(film.info.usage, preset.usage, "{}", preset.film);
        assert_eq!(
            film.info.reference_illuminant, preset.reference_illuminant,
            "{}",
            preset.film
        );
        let print = film.clone();
        let params = RuntimeParams::default();
        let pipeline = Pipeline::new(film, print, params.clone()).with_params(params);
        let got = &pipeline.params.film_render.dir_couplers;
        let mut want = preset.dir_couplers.clone();
        if preset.channel_model == "bw" {
            broadcast_bw(&mut want);
        }
        assert_eq!(
            serde_json::to_value(got).unwrap(),
            serde_json::to_value(&want).unwrap(),
            "{}",
            preset.film
        );
    }
}
