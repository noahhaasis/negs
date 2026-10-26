//! Experimental's grain against Python 28bf883 (ticket 16, item 3).
//!
//! The fixture comes from `scripts/parity/freeze_grain_statistics.py`, never
//! from Rust. The presets and the deterministic parts (sub-layer floors and
//! maxima, particle area from the RMS granularity, grains per pixel, the
//! sub-layer interpolation, the multiplicative unsharp mask) must match
//! exactly. The grain itself is random and is gated statistically.
//!
//! Gates. Python grained `reps` independent flat fields per case; the fixture
//! keeps the mean μ and sample standard deviation σ of each statistic over
//! them. Rust (CPU or GPU) grains one field of the same size, so its value has
//! the same spread, and `|rust − μ| ≤ K·σ·√(1 + 1/reps) + floor` with K = 5.
//! That is a 5σ two-sided test (a false failure about once in 1.7 million per
//! comparison, so about 1 in 2000 over the ~900 comparisons of a run). The
//! floor covers the rounding of the fixture and Python's own sampler choice.
//! The RMS and autocorrelation are gated against `use_fast_stats=True`
//! (experimental's default), whose normal approximations differ only slightly
//! from the exact samplers Rust uses. The mean shift is gated against
//! `use_fast_stats=False` instead: numba's `fast_binomial` inverts the CDF
//! from `(1 − p)^n`, which underflows to 0 for many grains at high
//! development, and then returns n (every grain developed). That biases the
//! fast mean density up by up to ~0.007 in the denser fixture cases; numpy's
//! exact sampler, which Rust reproduces, has no such bias. The floors are
//! 1 % of the RMS, 0.01 of an autocorrelation and 0.05 (× 1000 density) of a
//! mean shift; the test prints how far each statistic is in σ.

use serde_json::Value;
use spektrafilm_core::{grain, params::GrainParams, profile};
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{Scalar, from_f64, to_f64};
use spektrafilm_model::grain::{
    GrainCurves, GrainSettings, LayerInterp, apply_grain, layer_density, layer_grain_params,
    multiplicative_unsharp_mask,
};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const K_SIGMA: f64 = 5.0;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture() -> &'static Value {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/grain_python_28bf883.json");
        let value: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["reference_commit"], "28bf883e1672e884307edc75852549376e13644e");
        value
    })
}

fn f64s(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

/// A per-channel value of a B&W (one-channel) case repeats channel 0.
fn lanes(values: &[f64]) -> [f64; 3] {
    std::array::from_fn(|ch| values[ch.min(values.len() - 1)])
}

fn per_channel(v: &Value) -> [f64; 3] {
    let values: Vec<f64> = v.as_array().unwrap().iter().filter_map(Value::as_f64).collect();
    lanes(&values)
}

struct Case {
    curves: GrainCurves,
    settings: GrainSettings,
    positive: bool,
    bw: bool,
    n_channels: usize,
}

fn case(stock: &str) -> Case {
    let s = &fixture()["stocks"][stock];
    let n_channels = s["n_channels"].as_u64().unwrap() as usize;
    let total: Vec<[f64; 3]> = s["curves_total"].as_array().unwrap().iter().map(|r| lanes(&f64s(r))).collect();
    let layers: Vec<Vec<[f64; 3]>> = s["curves_layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row.as_array().unwrap().iter().map(|l| lanes(&f64s(l))).collect())
        .collect();
    let g = &s["grain"];
    let settings = GrainSettings {
        rms_granularity: per_channel(&g["rms_granularity"]),
        density_min: per_channel(&g["density_min"]),
        uniformity: per_channel(&g["uniformity"]),
        particle_scale_sublayers: per_channel(&g["particle_scale_sublayers"]),
        blur: g["blur"].as_f64().unwrap(),
        mult_usm_sigma: g["mult_usm_sigma"].as_f64().unwrap(),
        mult_usm_amount: g["mult_usm_amount"].as_f64().unwrap(),
        blur_dye_clouds_um: g["blur_dye_clouds_um"].as_f64().unwrap(),
        micro_structure: lanes(&f64s(&g["micro_structure"]))[..2].try_into().unwrap(),
    };
    Case {
        curves: GrainCurves::new(total, &layers),
        settings,
        positive: s["type"] == "positive",
        bw: n_channels == 1,
        n_channels,
    }
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-12) || (a - b).abs() <= 1e-15
}

#[test]
fn presets_match_python() {
    let data = root().join("data");
    let presets = fixture()["presets"].as_object().unwrap();
    assert!(presets.len() >= 20, "{} stocks", presets.len());
    for (stock, expected) in presets {
        let film = profile::resolve_for_render(profile::load_profile_by_name(&data, stock).unwrap(), None);
        let s = grain::grain_settings(&film, &GrainParams::default());
        let n = if film.is_bw() { 1 } else { 3 };
        for (name, got) in [
            ("rms_granularity", s.rms_granularity),
            ("density_min", s.density_min),
            ("uniformity", s.uniformity),
            ("particle_scale_sublayers", s.particle_scale_sublayers),
        ] {
            let want: Vec<Option<f64>> = expected[name].as_array().unwrap().iter().map(Value::as_f64).collect();
            let n = if name == "particle_scale_sublayers" { 3 } else { n };
            for ch in 0..n {
                assert_eq!(Some(got[ch]), want[ch], "{stock} {name}[{ch}]");
            }
        }
    }
}

#[test]
fn deterministic_parts_match_python() {
    for (stock, s) in fixture()["stocks"].as_object().unwrap() {
        let c = case(stock);
        for (pixel, d) in s["deterministic"].as_object().unwrap() {
            let lp = layer_grain_params(&c.curves, &c.settings, pixel.parse().unwrap());
            let check = |name: &str, got: &dyn Fn(usize, usize) -> f64| {
                for (l, row) in d[name].as_array().unwrap().iter().enumerate() {
                    for (ch, want) in f64s(row).into_iter().enumerate() {
                        let v = got(l, ch);
                        assert!(close(v, want, 1e-9), "{stock} {pixel} µm {name}[{l}][{ch}]: {v} vs {want}");
                    }
                }
            };
            check("density_min_layers", &|l, ch| lp.density_min_layers[l][ch]);
            check("density_max_layers", &|l, ch| lp.density_max_layers[l][ch]);
            check("n_particles_per_pixel", &|l, ch| lp.n_particles_per_pixel[l][ch]);
            check("dye_cloud_sigma_px", &|l, ch| {
                lp.dye_cloud_sigma(c.settings.blur_dye_clouds_um, l, ch)
            });
            for (ch, want) in f64s(&d["coarsest_area_um2"]).into_iter().enumerate() {
                let v = lp.coarsest_area_um2[ch];
                assert!(close(v, want, 1e-9), "{stock} {pixel} µm area[{ch}]: {v} vs {want}");
            }
        }
        // Sub-layer interpolation, `[channel][probe][layer]`.
        let probe = f64s(&s["interp"]["density"]);
        for (ch, rows) in s["interp"]["layers"].as_array().unwrap().iter().enumerate() {
            let interp = LayerInterp::new(&c.curves, ch, c.positive);
            for (d, row) in probe.iter().zip(rows.as_array().unwrap()) {
                let pos = interp.locate(*d);
                for (l, want) in f64s(row).into_iter().enumerate() {
                    let v = layer_density(&c.curves, pos, l, ch);
                    assert!((v - want).abs() <= 1e-12, "{stock} interp ch {ch} d {d} layer {l}: {v} vs {want}");
                }
            }
        }
    }
}

#[test]
fn multiplicative_unsharp_mask_matches_python() {
    let usm = &fixture()["usm"];
    let shape: Vec<usize> = usm["shape"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize).collect();
    let (h, w) = (shape[0], shape[1]);
    let input = f64s(&usm["input"]);
    // f32 rounds the pow() of values up to ~2.5. The f64 build differs only
    // because `gaussian_blur_channel` takes σ as f32 (0.7 becomes
    // 0.699999988), which moves the kernel weights by ~1e-8.
    let tol = if std::mem::size_of::<Scalar>() == 8 { 1e-7 } else { 2e-5 };
    for case in usm["cases"].as_array().unwrap() {
        let (sigma, amount) = (case["sigma"].as_f64().unwrap(), case["amount"].as_f64().unwrap());
        let output = f64s(&case["output"]);
        for ch in 0..3 {
            let mut plane: Vec<Scalar> = (0..w * h).map(|i| from_f64(input[i * 3 + ch])).collect();
            multiplicative_unsharp_mask(&mut plane, w as u32, h as u32, sigma, amount);
            for (i, &v) in plane.iter().enumerate() {
                let want = output[i * 3 + ch];
                assert!(
                    (to_f64(v) - want).abs() <= tol * want.abs().max(1.0),
                    "σ {sigma} amount {amount} ch {ch} px {i}: {} vs {want}",
                    to_f64(v)
                );
            }
        }
    }
}

/// The fixture's statistics of one grained flat field (see the freeze
/// script's `field_statistics`), per channel.
fn field_statistics(out: &ImageBuf, level: f64, pixel_size_um: f64, n_channels: usize, lags: &[usize]) -> Value {
    let (w, h) = (out.width as usize, out.height as usize);
    let boxed = (48.0 / pixel_size_um).round() as usize;
    let mut stats = serde_json::Map::new();
    let mut pixel_rms = Vec::new();
    let mut aperture_rms = Vec::new();
    let mut mean_shift = Vec::new();
    let mut acf: Vec<Vec<f64>> = vec![Vec::new(); lags.len()];
    for ch in 0..n_channels {
        let noise: Vec<f64> = out.data.iter().skip(ch).step_by(3).map(|&v| to_f64(v) - level).collect();
        let n = noise.len() as f64;
        let mean = noise.iter().sum::<f64>() / n;
        let var = noise.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let (bw, bh) = (w / boxed, h / boxed);
        let boxes: Vec<f64> = (0..bh * bw)
            .map(|b| {
                let (by, bx) = (b / bw, b % bw);
                let mut s = 0.0;
                for y in by * boxed..(by + 1) * boxed {
                    for x in bx * boxed..(bx + 1) * boxed {
                        s += noise[y * w + x];
                    }
                }
                s / (boxed * boxed) as f64
            })
            .collect();
        let bm = boxes.iter().sum::<f64>() / boxes.len() as f64;
        let bvar = boxes.iter().map(|v| (v - bm).powi(2)).sum::<f64>() / boxes.len() as f64;
        pixel_rms.push(var.sqrt() * 1000.0);
        aperture_rms.push(bvar.sqrt() * 1000.0);
        mean_shift.push(mean * 1000.0);
        let c: Vec<f64> = noise.iter().map(|v| v - mean).collect();
        for (i, &lag) in lags.iter().enumerate() {
            let (mut hs, mut vs) = (0.0, 0.0);
            for y in 0..h {
                for x in 0..w - lag {
                    hs += c[y * w + x + lag] * c[y * w + x];
                }
            }
            for y in 0..h - lag {
                for x in 0..w {
                    vs += c[(y + lag) * w + x] * c[y * w + x];
                }
            }
            let hs = hs / (h * (w - lag)) as f64;
            let vs = vs / ((h - lag) * w) as f64;
            acf[i].push(0.5 * (hs + vs) / var);
        }
    }
    stats.insert("pixel_rms".into(), pixel_rms.into());
    stats.insert("aperture_rms".into(), aperture_rms.into());
    stats.insert("mean_shift".into(), mean_shift.into());
    let acf_map: serde_json::Map<String, Value> =
        lags.iter().zip(acf).map(|(lag, v)| (lag.to_string(), v.into())).collect();
    stats.insert("acf".into(), acf_map.into());
    Value::Object(stats)
}

/// Gate one statistic; returns the worst |z| (in σ) for the report.
fn gate(label: &str, got: &[f64], reference: &Value, floor: &dyn Fn(f64) -> f64, reps: f64, failures: &mut Vec<String>) -> f64 {
    let (mean, std) = (f64s(&reference["mean"]), f64s(&reference["std"]));
    let mut worst: f64 = 0.0;
    for ch in 0..mean.len() {
        let spread = std[ch] * (1.0 + 1.0 / reps).sqrt();
        let tol = K_SIGMA * spread + floor(mean[ch]);
        let diff = got[ch] - mean[ch];
        worst = worst.max(diff.abs() / spread.max(1e-12));
        if diff.abs() > tol {
            failures.push(format!(
                "{label}[{ch}]: {:.4} vs Python {:.4} ± {:.4} (tol {:.4})",
                got[ch], mean[ch], std[ch], tol
            ));
        }
    }
    worst
}

type GrainFn<'a> = dyn Fn(ImageBuf, f64) -> ImageBuf + 'a;

fn check_statistics(stock: &str, engine: &str, run: &GrainFn<'_>) {
    let fx = fixture();
    let design = &fx["design"];
    let size = design["field_size_px"].as_u64().unwrap() as u32;
    let reps = design["reps"].as_f64().unwrap();
    let lags: Vec<usize> = design["acf_lags"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize).collect();
    let c = case(stock);
    let mut failures = Vec::new();
    for (pixel, by_level) in fx["stocks"][stock]["statistics"].as_object().unwrap() {
        let pixel_size: f64 = pixel.parse().unwrap();
        for (level, modes) in by_level.as_object().unwrap() {
            let level_f: f64 = level.parse().unwrap();
            let v = from_f64(level_f);
            let field = ImageBuf::from_data(size, size, vec![v; (size * size * 3) as usize]);
            let out = run(field, pixel_size);
            let got = field_statistics(&out, level_f, pixel_size, c.n_channels, &lags);
            let reference = &modes["fast"];
            let label = format!("{engine} {stock} {pixel} µm D {level}");
            let rms_floor = |m: f64| 0.01 * m.abs();
            let mut z = Vec::new();
            for name in ["pixel_rms", "aperture_rms"] {
                z.push(gate(&format!("{label} {name}"), &f64s(&got[name]), &reference[name], &rms_floor, reps, &mut failures));
            }
            let exact = &modes["exact"];
            z.push(gate(&format!("{label} mean_shift"), &f64s(&got["mean_shift"]), &exact["mean_shift"], &|_| 0.05, reps, &mut failures));
            for lag in &lags {
                let key = lag.to_string();
                z.push(gate(
                    &format!("{label} acf[{lag}]"),
                    &f64s(&got["acf"][&key]),
                    &reference["acf"][&key],
                    &|_| 0.01,
                    reps,
                    &mut failures,
                ));
            }
            eprintln!(
                "{label}: pixel rms {:?} (Python {:?}), 48 µm rms {:?} (Python {:?}), worst |z| per stat {:?}",
                f64s(&got["pixel_rms"]).iter().map(|v| (v * 100.0).round() / 100.0).collect::<Vec<_>>(),
                f64s(&reference["pixel_rms"]["mean"]).iter().map(|v| (v * 100.0).round() / 100.0).collect::<Vec<_>>(),
                f64s(&got["aperture_rms"]).iter().map(|v| (v * 100.0).round() / 100.0).collect::<Vec<_>>(),
                f64s(&reference["aperture_rms"]["mean"]).iter().map(|v| (v * 100.0).round() / 100.0).collect::<Vec<_>>(),
                z.iter().map(|v| (v * 10.0).round() / 10.0).collect::<Vec<_>>(),
            );
        }
    }
    assert!(failures.is_empty(), "{} statistics outside the gates:\n{}", failures.len(), failures.join("\n"));
}

fn cpu(stock: &str) {
    let c = case(stock);
    check_statistics(stock, "cpu", &|field, pixel| {
        apply_grain(field, pixel, &c.settings, &c.curves, c.positive, c.bw, false)
    });
}

fn gpu(stock: &str) {
    let Some(backend) = spektrafilm_gpu::wgpu_backend::WgpuBackend::new() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let c = case(stock);
    check_statistics(stock, "gpu", &|field, pixel| {
        let gp = spektrafilm_model::grain::gpu_params(&c.curves, &c.settings, pixel, c.positive, c.bw);
        backend.run_grain(&field, &gp)
    });
}

#[test]
fn cpu_statistics_portra_400() {
    cpu("kodak_portra_400");
}
#[test]
fn cpu_statistics_vision3_500t() {
    cpu("kodak_vision3_500t");
}
#[test]
fn cpu_statistics_provia_100f() {
    cpu("fujifilm_provia_100f");
}
#[test]
fn cpu_statistics_doublex() {
    cpu("kodak_doublex");
}
#[test]
fn cpu_statistics_trix() {
    cpu("kodak_trix");
}
#[test]
fn gpu_statistics_portra_400() {
    gpu("kodak_portra_400");
}
#[test]
fn gpu_statistics_vision3_500t() {
    gpu("kodak_vision3_500t");
}
#[test]
fn gpu_statistics_provia_100f() {
    gpu("fujifilm_provia_100f");
}
#[test]
fn gpu_statistics_doublex() {
    gpu("kodak_doublex");
}
#[test]
fn gpu_statistics_trix() {
    gpu("kodak_trix");
}
