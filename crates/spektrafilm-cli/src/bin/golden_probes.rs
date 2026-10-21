// Fixed-input probes for the cross-platform golden test: the platform math
// functions the engine calls, and the numpy-compatible grain samplers.
//
// Usage:
//   golden_probes <out_dir>
//
// Writes one little-endian binary file per probe (`math_*.f64`, `math_*.f32`,
// `grain_*.u64`) whose bytes must be identical on every platform. A
// difference in a `math_*` file names the platform function that differs; a
// difference in a `grain_*` file shows where a sampler took another branch or
// consumed a different number of random draws. Each grain file ends with one
// extra raw draw, so a change in draw consumption shows up even if the
// samples agree.

use std::path::Path;

use anyhow::{Result, bail};
use rand_mt::Mt;
use spektrafilm_math::numpy_rng::{rk_binomial, rk_poisson};

// `vforce` declares `vvpow` but leaves linking Accelerate to the BLAS
// crates, which this binary does not pull in.
#[cfg(target_vendor = "apple")]
#[link(name = "Accelerate", kind = "framework")]
unsafe extern "C" {}

const N: usize = 1 << 16;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [out] = args.as_slice() else {
        bail!("usage: golden_probes <out_dir>");
    };
    let out = Path::new(out);
    std::fs::create_dir_all(out)?;

    let mut rnd = XorShift(0x9E37_79B9_7F4A_7C15);
    let densities: Vec<f64> = (0..N).map(|_| rnd.uniform() * 7.0 - 3.0).collect();
    let positive: Vec<f64> = (0..N).map(|_| rnd.uniform() * 8.0 + 1e-9).collect();
    let small: Vec<f64> = (0..N).map(|_| rnd.uniform() * 2.0 - 1.0).collect();
    let ev: Vec<f32> = (0..N).map(|_| (rnd.uniform() * 12.0 - 6.0) as f32).collect();

    // f64 functions on the shapes the engine uses.
    let f64_probes: [(&str, Box<dyn Fn(usize) -> f64>); 10] = [
        ("pow10", Box::new(|i| 10f64.powf(densities[i]))),
        ("pow", Box::new(|i| positive[i].powf(small[i] * 3.0))),
        ("exp", Box::new(|i| densities[i].exp())),
        ("ln", Box::new(|i| positive[i].ln())),
        ("log10", Box::new(|i| positive[i].log10())),
        ("atan2", Box::new(|i| small[i].atan2(positive[i] - 4.0))),
        ("cbrt", Box::new(|i| positive[i].cbrt())),
        ("sin", Box::new(|i| (densities[i] * 3.0).sin())),
        ("cos", Box::new(|i| (densities[i] * 3.0).cos())),
        ("exp_neg_lambda", Box::new(|i| (-positive[i] * 4.0).exp())),
    ];
    for (name, f) in &f64_probes {
        let v: Vec<f64> = (0..N).map(f).collect();
        write(out, &format!("math_{name}.f64"), v.iter().flat_map(|x| x.to_le_bytes()))?;
    }
    // The engine's 10^x helper (vForce `vvpow` on Apple, `powf` elsewhere).
    let mut v = densities.clone();
    spektrafilm_math::vforce::exp10_inplace(&mut v);
    write(out, "math_exp10_inplace.f64", v.iter().flat_map(|x| x.to_le_bytes()))?;

    // f32 functions (auto-exposure and exposure compensation use 2f32.powf).
    let f32_probes: [(&str, Box<dyn Fn(usize) -> f32>); 4] = [
        ("pow2", Box::new(|i| 2f32.powf(ev[i]))),
        ("powf", Box::new(|i| (positive[i] as f32).powf(small[i] as f32 * 3.0))),
        ("exp", Box::new(|i| (densities[i] as f32).exp())),
        ("log10", Box::new(|i| (positive[i] as f32).log10())),
    ];
    for (name, f) in &f32_probes {
        let v: Vec<f32> = (0..N).map(f).collect();
        write(out, &format!("math_{name}.f32"), v.iter().flat_map(|x| x.to_le_bytes()))?;
    }

    // Poisson: the multiplication sampler below λ = 10, PTRS at and above it,
    // a dense grid across the switch, and the grain path's typical rates.
    let mut lambdas: Vec<f64> = (0..20_000).map(|i| i as f64 * 0.001).collect();
    let mut x = 10.0f64;
    for _ in 0..64 {
        x = f64::from_bits(x.to_bits() - 1);
    }
    for _ in 0..128 {
        lambdas.push(x);
        x = f64::from_bits(x.to_bits() + 1);
    }
    lambdas.extend((0..N).map(|_| rnd.uniform() * 50.0));
    poisson(out, "grain_poisson", 0, &lambdas)?;

    // Binomial: inversion for n·p ≤ 30, BTPE above, both sides of p = 0.5.
    let mut cases: Vec<(u64, f64)> = Vec::new();
    for (n, p) in [(60u64, 0.5f64), (100, 0.3), (61, 0.5), (200, 0.15)] {
        let mut q = p;
        for _ in 0..64 {
            q = f64::from_bits(q.to_bits() - 1);
        }
        for _ in 0..128 {
            cases.push((n, q));
            q = f64::from_bits(q.to_bits() + 1);
        }
    }
    cases.extend((0..N).map(|_| ((rnd.uniform() * 400.0) as u64, rnd.uniform())));
    binomial(out, "grain_binomial", 0, &cases)?;
    Ok(())
}

struct XorShift(u64);

impl XorShift {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn poisson(out: &Path, name: &str, seed: u32, lambdas: &[f64]) -> Result<()> {
    let mut rng = Mt::new(seed);
    let mut v: Vec<u64> = lambdas.iter().map(|&l| rk_poisson(&mut rng, l)).collect();
    v.push(rng.next_u32() as u64);
    write(out, &format!("{name}.u64"), v.iter().flat_map(|x| x.to_le_bytes()))
}

fn binomial(out: &Path, name: &str, seed: u32, cases: &[(u64, f64)]) -> Result<()> {
    let mut rng = Mt::new(seed);
    let mut v: Vec<u64> = cases.iter().map(|&(n, p)| rk_binomial(&mut rng, n, p)).collect();
    v.push(rng.next_u32() as u64);
    write(out, &format!("{name}.u64"), v.iter().flat_map(|x| x.to_le_bytes()))
}

fn write(out: &Path, name: &str, bytes: impl Iterator<Item = u8>) -> Result<()> {
    std::fs::write(out.join(name), bytes.collect::<Vec<u8>>())?;
    Ok(())
}
