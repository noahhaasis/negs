// Compare two raw f64 output dumps (`--raw-out`) of the golden test against
// a named gate profile, and optionally the 16-bit TIFFs saved from them.
//
// Usage:
//   golden_compare --gates GATES.json --profile NAME [--scalar f32|f64]
//       A.f64 B.f64 WIDTH HEIGHT [--tiff-a A.tiff --tiff-b B.tiff]
//
// Prints one JSON object with the statistics, the gates applied, the failed
// gates and `pass`; exits 0 on pass, 1 on fail, 2 on usage or I/O errors.
//
// Per-value gate: every value satisfies |a−b| ≤ value_abs + value_rel·max(|a|,|b|).
// Colour differences are CIEDE2000 on the clipped, sRGB-encoded output (what the
// saved file holds), with the same sRGB matrix and D65 white as
// `scripts/gpu_cpu_quality/compare.py` (colour-science). Each TIFF must equal
// the quantisation of its own dump (`save.rs`: clamp, ×65535, ties to even, in
// the engine's scalar type), so a code difference between the TIFFs is always
// explained by the raw difference that the per-value gate bounds.

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value, json};

const USAGE: &str = "usage: golden_compare --gates GATES.json --profile NAME [--scalar f32|f64] \
A.f64 B.f64 WIDTH HEIGHT [--tiff-a A.tiff --tiff-b B.tiff]";

fn main() {
    match run() {
        Ok(pass) => std::process::exit(if pass { 0 } else { 1 }),
        Err(e) => {
            eprintln!("golden_compare: {e:#}");
            std::process::exit(2);
        }
    }
}

struct Args {
    gates: String,
    profile: String,
    f32_scalar: bool,
    a: String,
    b: String,
    width: usize,
    height: usize,
    tiff_a: Option<String>,
    tiff_b: Option<String>,
}

fn parse_args() -> Result<Args> {
    let mut gates = None;
    let mut profile = None;
    let mut f32_scalar = false;
    let mut tiff_a = None;
    let mut tiff_b = None;
    let mut positional = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| anyhow!(USAGE));
        match arg.as_str() {
            "--gates" => gates = Some(value()?),
            "--profile" => profile = Some(value()?),
            "--scalar" => {
                f32_scalar = match value()?.as_str() {
                    "f32" => true,
                    "f64" => false,
                    other => bail!("--scalar must be f32 or f64, not {other:?}"),
                }
            }
            "--tiff-a" => tiff_a = Some(value()?),
            "--tiff-b" => tiff_b = Some(value()?),
            _ => positional.push(arg),
        }
    }
    let [a, b, w, h] = <[String; 4]>::try_from(positional).map_err(|_| anyhow!(USAGE))?;
    if tiff_a.is_some() != tiff_b.is_some() {
        bail!("--tiff-a and --tiff-b go together");
    }
    Ok(Args {
        gates: gates.ok_or_else(|| anyhow!(USAGE))?,
        profile: profile.ok_or_else(|| anyhow!(USAGE))?,
        f32_scalar,
        a,
        b,
        width: w.parse().context("WIDTH")?,
        height: h.parse().context("HEIGHT")?,
        tiff_a,
        tiff_b,
    })
}

fn run() -> Result<bool> {
    let args = parse_args()?;
    let all_gates: Value = serde_json::from_slice(
        &std::fs::read(&args.gates).with_context(|| format!("reading {}", args.gates))?,
    )?;
    let gates = all_gates
        .get(&args.profile)
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("no gate profile {:?} in {}", args.profile, args.gates))?
        .clone();
    let a = read_f64(&args.a)?;
    let b = read_f64(&args.b)?;
    let (w, h) = (args.width, args.height);

    let mut stats = Map::new();
    let mut failures: Vec<String> = Vec::new();
    let expected = w * h * 3;
    if a.len() != expected || b.len() != expected {
        failures.push(format!("size: {} and {} values, expected {expected}", a.len(), b.len()));
        print(&args.profile, stats, &gates, failures);
        return Ok(false);
    }

    let nonfinite = |v: &[f64]| v.iter().filter(|x| !x.is_finite()).count();
    let (bad_a, bad_b) = (nonfinite(&a), nonfinite(&b));
    stats.insert("nonfinite".into(), json!([bad_a, bad_b]));
    if bad_a + bad_b > 0 {
        failures.push(format!("non-finite values: {bad_a} in A, {bad_b} in B"));
    }

    values(&a, &b, w, &gates, &mut stats, &mut failures);
    colour(&a, &b, w, &mut stats);
    codes(&a, &b, args.f32_scalar, &mut stats);
    if let (Some(ta), Some(tb)) = (&args.tiff_a, &args.tiff_b) {
        tiffs(ta, tb, &a, &b, w, h, args.f32_scalar, &mut stats, &mut failures)?;
    }
    check_limits(&gates, &stats, &mut failures);

    let pass = failures.is_empty();
    print(&args.profile, stats, &gates, failures);
    Ok(pass)
}

fn print(profile: &str, stats: Map<String, Value>, gates: &Map<String, Value>, failures: Vec<String>) {
    let out = json!({
        "profile": profile,
        "pass": failures.is_empty(),
        "failures": failures,
        "gates": gates,
        "stats": stats,
    });
    println!("{out}");
}

fn read_f64(path: &str) -> Result<Vec<f64>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {path}"))?;
    if bytes.len() % 8 != 0 {
        bail!("{path}: {} bytes is not a whole number of f64s", bytes.len());
    }
    Ok(bytes
        .chunks_exact(8)
        .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
        .collect())
}

fn gate(gates: &Map<String, Value>, key: &str) -> Option<f64> {
    gates.get(key).and_then(Value::as_f64)
}

/// Per-value differences and the per-value gate.
fn values(
    a: &[f64],
    b: &[f64],
    w: usize,
    gates: &Map<String, Value>,
    stats: &mut Map<String, Value>,
    failures: &mut Vec<String>,
) {
    let abs_tol = gate(gates, "value_abs");
    let rel_tol = gate(gates, "value_rel");
    let mut differ = 0usize;
    let mut max_abs = 0.0f64;
    let mut max_abs_at = 0usize;
    let mut sum_sq = 0.0f64;
    let mut worst = 0.0f64;
    let mut worst_at = 0usize;
    let mut violations = 0usize;
    for (i, (&x, &y)) in a.iter().zip(b).enumerate() {
        if x.to_bits() == y.to_bits() {
            continue;
        }
        differ += 1;
        let d = (x - y).abs();
        let m = x.abs().max(y.abs());
        if d > max_abs || d.is_nan() {
            max_abs = d;
            max_abs_at = i;
        }
        let n = d / (1.0 + m);
        sum_sq += n * n;
        if let (Some(at), Some(rt)) = (abs_tol, rel_tol) {
            let excess = d / (at + rt * m);
            if excess > 1.0 || excess.is_nan() {
                violations += 1;
            }
            if excess > worst || excess.is_nan() {
                worst = excess;
                worst_at = i;
            }
        }
    }
    let at = |i: usize| json!({"x": (i / 3) % w, "y": (i / 3) / w, "c": i % 3});
    stats.insert("values".into(), json!(a.len()));
    stats.insert("differ".into(), json!(differ));
    stats.insert("max_abs".into(), json!(max_abs));
    stats.insert("max_abs_at".into(), at(max_abs_at));
    stats.insert("rms_norm".into(), json!((sum_sq / a.len() as f64).sqrt()));
    if abs_tol.is_some() && rel_tol.is_some() {
        stats.insert("value_gate_worst".into(), json!(worst));
        stats.insert("value_gate_worst_at".into(), at(worst_at));
        stats.insert("value_gate_violations".into(), json!(violations));
        if violations > 0 {
            failures.push(format!(
                "{violations} values exceed value_abs + value_rel·max (worst ×{worst:.3e})"
            ));
        }
    }
}

/// CIEDE2000 between the two outputs, per pixel.
fn colour(a: &[f64], b: &[f64], w: usize, stats: &mut Map<String, Value>) {
    let mut de: Vec<f64> = a
        .chunks_exact(3)
        .zip(b.chunks_exact(3))
        .map(|(p, q)| {
            if p == q {
                0.0
            } else {
                ciede2000(lab(p), lab(q))
            }
        })
        .collect();
    let (max_i, max) = de
        .iter()
        .enumerate()
        .fold((0, 0.0f64), |acc, (i, &v)| if v > acc.1 { (i, v) } else { acc });
    let n = de.len() as f64;
    let mean = de.iter().sum::<f64>() / n;
    let gt = |t: f64| de.iter().filter(|&&v| v > t).count() as f64 / n;
    let (gt05, gt1, gt2) = (gt(0.5), gt(1.0), gt(2.0));
    de.sort_by(f64::total_cmp);
    stats.insert("de00_mean".into(), json!(mean));
    stats.insert("de00_p99".into(), json!(percentile(&de, 99.0)));
    stats.insert("de00_p999".into(), json!(percentile(&de, 99.9)));
    stats.insert("de00_max".into(), json!(max));
    stats.insert("de00_max_at".into(), json!({"x": max_i % w, "y": max_i / w}));
    stats.insert("frac_de00_gt_0_5".into(), json!(gt05));
    stats.insert("frac_de00_gt_1".into(), json!(gt1));
    stats.insert("frac_de00_gt_2".into(), json!(gt2));
}

/// numpy's default (linear) percentile of sorted values.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    let pos = p / 100.0 * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

/// Lab (D65) of a clipped, sRGB-encoded pixel.
fn lab(rgb: &[f64]) -> [f64; 3] {
    // colour-science's sRGB normalised primary matrix and D65 (x 0.3127, y 0.3290).
    const M: [[f64; 3]; 3] = [
        [0.412_390_799_265_959_5, 0.357_584_339_383_878, 0.180_480_788_401_834_3],
        [0.212_639_005_871_510_36, 0.715_168_678_767_756, 0.072_192_315_360_733_71],
        [0.019_330_818_715_591_82, 0.119_194_779_794_625_98, 0.950_532_152_249_660_7],
    ];
    const WHITE: [f64; 3] = [0.3127 / 0.3290, 1.0, (1.0 - 0.3127 - 0.3290) / 0.3290];
    let lin = |c: f64| {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let l = [lin(rgb[0]), lin(rgb[1]), lin(rgb[2])];
    let f = |t: f64| {
        const E: f64 = 216.0 / 24389.0;
        const K: f64 = 24389.0 / 27.0;
        if t > E { t.cbrt() } else { (K * t + 16.0) / 116.0 }
    };
    let xyz: Vec<f64> = (0..3)
        .map(|r| (M[r][0] * l[0] + M[r][1] * l[1] + M[r][2] * l[2]) / WHITE[r])
        .collect();
    let (fx, fy, fz) = (f(xyz[0]), f(xyz[1]), f(xyz[2]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIEDE2000 (Sharma, Wu & Dalal 2005), kL = kC = kH = 1.
fn ciede2000(p: [f64; 3], q: [f64; 3]) -> f64 {
    use std::f64::consts::PI;
    let deg = |r: f64| r * 180.0 / PI;
    let rad = |d: f64| d * PI / 180.0;
    let (l1, a1, b1) = (p[0], p[1], p[2]);
    let (l2, a2, b2) = (q[0], q[1], q[2]);
    let c_bar = ((a1 * a1 + b1 * b1).sqrt() + (a2 * a2 + b2 * b2).sqrt()) / 2.0;
    let g = 0.5 * (1.0 - (c_bar.powi(7) / (c_bar.powi(7) + 25f64.powi(7))).sqrt());
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let (c1p, c2p) = ((a1p * a1p + b1 * b1).sqrt(), (a2p * a2p + b2 * b2).sqrt());
    let hue = |b: f64, a: f64| {
        if a == 0.0 && b == 0.0 {
            0.0
        } else {
            let h = deg(b.atan2(a));
            if h < 0.0 { h + 360.0 } else { h }
        }
    };
    let (h1p, h2p) = (hue(b1, a1p), hue(b2, a2p));
    let dlp = l2 - l1;
    let dcp = c2p - c1p;
    let dhp = if c1p * c2p == 0.0 {
        0.0
    } else if (h2p - h1p).abs() <= 180.0 {
        h2p - h1p
    } else if h2p - h1p > 180.0 {
        h2p - h1p - 360.0
    } else {
        h2p - h1p + 360.0
    };
    let dhp_big = 2.0 * (c1p * c2p).sqrt() * rad(dhp / 2.0).sin();
    let lp_bar = (l1 + l2) / 2.0;
    let cp_bar = (c1p + c2p) / 2.0;
    let hp_bar = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let t = 1.0 - 0.17 * rad(hp_bar - 30.0).cos() + 0.24 * rad(2.0 * hp_bar).cos()
        + 0.32 * rad(3.0 * hp_bar + 6.0).cos()
        - 0.20 * rad(4.0 * hp_bar - 63.0).cos();
    let d_theta = 30.0 * (-((hp_bar - 275.0) / 25.0).powi(2)).exp();
    let rc = 2.0 * (cp_bar.powi(7) / (cp_bar.powi(7) + 25f64.powi(7))).sqrt();
    let sl = 1.0 + 0.015 * (lp_bar - 50.0).powi(2) / (20.0 + (lp_bar - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cp_bar;
    let sh = 1.0 + 0.015 * cp_bar * t;
    let rt = -rad(2.0 * d_theta).sin() * rc;
    ((dlp / sl).powi(2) + (dcp / sc).powi(2) + (dhp_big / sh).powi(2)
        + rt * (dcp / sc) * (dhp_big / sh))
        .sqrt()
}

/// `save.rs` quantisation, in the engine's scalar type.
fn quantize(v: f64, levels: f64, f32_scalar: bool) -> u32 {
    if f32_scalar {
        let v = v as f32;
        (v.clamp(0.0, 1.0) * levels as f32).round_ties_even() as u32
    } else {
        (v.clamp(0.0, 1.0) * levels).round_ties_even() as u32
    }
}

fn codes(a: &[f64], b: &[f64], f32_scalar: bool, stats: &mut Map<String, Value>) {
    for (name, levels) in [("code8", 255.0), ("code16", 65535.0)] {
        let mut max = 0u32;
        let mut differ = 0usize;
        for (&x, &y) in a.iter().zip(b) {
            let d = quantize(x, levels, f32_scalar).abs_diff(quantize(y, levels, f32_scalar));
            if d > 0 {
                differ += 1;
                max = max.max(d);
            }
        }
        stats.insert(format!("{name}_max"), json!(max));
        stats.insert(format!("{name}_differ"), json!(differ));
    }
}

fn read_tiff_u16(path: &str) -> Result<(u32, u32, Vec<u16>)> {
    use tiff::decoder::{Decoder, DecodingResult};
    let file = std::fs::File::open(Path::new(path)).with_context(|| format!("opening {path}"))?;
    let mut dec = Decoder::new(std::io::BufReader::new(file))?;
    let (w, h) = dec.dimensions()?;
    match dec.read_image()? {
        DecodingResult::U16(v) => Ok((w, h, v)),
        _ => bail!("{path}: not a 16-bit TIFF"),
    }
}

#[allow(clippy::too_many_arguments)]
fn tiffs(
    ta: &str,
    tb: &str,
    a: &[f64],
    b: &[f64],
    w: usize,
    h: usize,
    f32_scalar: bool,
    stats: &mut Map<String, Value>,
    failures: &mut Vec<String>,
) -> Result<()> {
    let mut pixels = Vec::new();
    for (path, raw, name) in [(ta, a, "A"), (tb, b, "B")] {
        let (tw, th, px) = read_tiff_u16(path)?;
        if (tw as usize, th as usize) != (w, h) || px.len() != raw.len() {
            failures.push(format!("TIFF {name} is {tw}×{th}, expected {w}×{h}"));
            return Ok(());
        }
        let mismatched = px
            .iter()
            .zip(raw)
            .filter(|&(&p, &v)| p as u32 != quantize(v, 65535.0, f32_scalar))
            .count();
        stats.insert(format!("tiff_{name}_vs_own_dump_mismatches"), json!(mismatched));
        if mismatched > 0 {
            failures.push(format!("TIFF {name}: {mismatched} codes differ from its own dump"));
        }
        pixels.push(px);
    }
    let (max, differ) = pixels[0]
        .iter()
        .zip(&pixels[1])
        .map(|(&p, &q)| p.abs_diff(q))
        .fold((0u16, 0usize), |(m, n), d| (m.max(d), n + (d > 0) as usize));
    stats.insert("tiff_code16_max".into(), json!(max));
    stats.insert("tiff_code16_differ".into(), json!(differ));
    Ok(())
}

/// Upper limits on statistics: a gate key `<stat>` caps stats[`<stat>`].
fn check_limits(gates: &Map<String, Value>, stats: &Map<String, Value>, failures: &mut Vec<String>) {
    for (key, limit) in gates {
        if matches!(key.as_str(), "value_abs" | "value_rel" | "description") {
            continue;
        }
        let Some(limit) = limit.as_f64() else { continue };
        match stats.get(key).and_then(Value::as_f64) {
            Some(v) if v > limit => failures.push(format!("{key} {v:.4e} > {limit:.4e}")),
            Some(_) => {}
            None => failures.push(format!("gate {key} has no statistic")),
        }
    }
}
