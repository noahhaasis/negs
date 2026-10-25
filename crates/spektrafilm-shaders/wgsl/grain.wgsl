// One emulsion sub-layer of upstream experimental's multilayer grain.
//
// CPU equivalent: `spektrafilm_model::grain::apply_grain` → the sub-layer
// interpolation (`LayerInterp`) and `layer_particle_model`. Poisson and
// binomial draws use the same exact algorithms as the CPU's numpy port
// (`spektrafilm_math::numpy_rng`): multiplication / PTRS for Poisson,
// inversion / BTPE for binomial. Only the uniform source differs: a per-pixel
// PCG32 stream instead of one MT19937, so the pattern differs from the CPU but
// its statistics do not. Same `base_seed` + same image dimensions reproduce
// the same noise.
//
// The host dispatches it once per sub-layer. Each pixel locates its density
// on the total curve, reads the sub-layer's density there, adds the
// sub-layer's floor, draws the grain and writes it to `out` (or adds it, with
// `accumulate`). The dye-cloud blur, the micro-structure, the pixel blur, the
// unsharp mask and the floor removal are later passes.

struct Params {
    n_pixels: u32,
    n_samples: u32,
    n_layers: u32,
    layer: u32,
    base_seed: u32,
    // 1 = B&W monochrome grain: all lanes share the channel-0 RNG stream,
    // producing one noise field (upstream n_channels==1 has a single
    // emulsion). 0 = independent per-channel streams (colour).
    monochrome: u32,
    accumulate: u32,
    // 1 = positive film: densities are negated to locate them on the
    // (negated, rising) total curve.
    positive: u32,
    density_min: vec4<f32>,            // this sub-layer's floor, .xyz
    density_max: vec4<f32>,            // this sub-layer's absolute maximum
    n_particles_per_pixel: vec4<f32>,  // 0 = noise-free (RMS granularity 0)
    grain_uniformity: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> density_cmy: array<f32>;
// [x (n_samples × 3)] [1/dx (n_samples × 3)] [layers (n_samples × n_layers × 3)]
@group(0) @binding(2) var<storage, read> curves: array<f32>;
@group(0) @binding(3) var<storage, read_write> out: array<f32>;

// SplitMix64 finalizer truncated to u32. Used to derive a per-pixel,
// per-channel, per-sublayer seed from the user `base_seed`.
fn splitmix32(x: u32) -> u32 {
    var z: u32 = x;
    z = (z ^ (z >> 16u)) * 0x85ebca6bu;
    z = (z ^ (z >> 13u)) * 0xc2b2ae35u;
    return z ^ (z >> 16u);
}

// PCG32 (32-bit state, RXS-M-XS output). Each pixel gets its own stream via
// an odd increment, so streams never share a sequence.
struct Rng {
    state: u32,
    inc: u32,
}

fn next_u32(rng: ptr<function, Rng>) -> u32 {
    let s = (*rng).state;
    (*rng).state = s * 747796405u + (*rng).inc;
    let word = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (word >> 22u) ^ word;
}

// Uniform in (0, 1) with 24 bits, never exactly 0 (logs stay finite).
fn uniform01(rng: ptr<function, Rng>) -> f32 {
    return (f32(next_u32(rng) >> 8u) + 0.5) * (1.0 / 16777216.0);
}

// ln(k!) — exact product below 8, Stirling series above (error < 1e-8).
fn log_factorial(k: f32) -> f32 {
    if k < 8.0 {
        var r = 1.0;
        for (var i = 2.0; i <= k; i += 1.0) {
            r *= i;
        }
        return log(r);
    }
    let x = k + 1.0;
    return (x - 0.5) * log(x) - x + 0.91893853320467274 + 1.0 / (12.0 * x) - 1.0 / (360.0 * x * x * x);
}

// numpy `rk_poisson`: multiplication of uniforms below λ = 10, Hörmann's PTRS
// transformed rejection from 10 on.
fn sample_poisson(lam: f32, rng: ptr<function, Rng>) -> f32 {
    if lam <= 0.0 {
        return 0.0;
    }
    if lam < 10.0 {
        let enlam = exp(-lam);
        var x = 0.0;
        var prod = 1.0;
        loop {
            prod *= uniform01(rng);
            if prod > enlam {
                x += 1.0;
            } else {
                return x;
            }
        }
    }
    let slam = sqrt(lam);
    let loglam = log(lam);
    let b = 0.931 + 2.53 * slam;
    let a = -0.059 + 0.02483 * b;
    let invalpha = 1.1239 + 1.1328 / (b - 3.4);
    let vr = 0.9277 - 3.6224 / (b - 2.0);
    loop {
        let u = uniform01(rng) - 0.5;
        let v = uniform01(rng);
        let us = 0.5 - abs(u);
        let k = floor((2.0 * a / us + b) * u + lam + 0.43);
        if us >= 0.07 && v <= vr {
            return k;
        }
        if k < 0.0 || (us < 0.013 && v > us) {
            continue;
        }
        let lhs = log(v) + log(invalpha) - log(a / (us * us) + b);
        let rhs = -lam + k * loglam - log_factorial(k);
        if lhs <= rhs {
            return k;
        }
    }
    return 0.0;
}

// numpy `rk_binomial_inversion` (n·p ≤ 30, p ≤ 0.5).
fn binomial_inversion(n: f32, p: f32, rng: ptr<function, Rng>) -> f32 {
    let q = 1.0 - p;
    let qn = exp(n * log(q));
    let np = n * p;
    let bound = floor(min(n, np + 10.0 * sqrt(np * q + 1.0)));
    var x = 0.0;
    var px = qn;
    var u = uniform01(rng);
    while u > px {
        x += 1.0;
        if x > bound {
            x = 0.0;
            px = qn;
            u = uniform01(rng);
        } else {
            u -= px;
            px = ((n - x + 1.0) * p * px) / (x * q);
        }
    }
    return x;
}

fn stirling_correction(x: f32) -> f32 {
    let x2 = x * x;
    return (13680.0 - (462.0 - (132.0 - (99.0 - 140.0 / x2) / x2) / x2) / x2) / x / 166320.0;
}

// numpy `rk_binomial_btpe` (Kachitvichyanukul & Schmeiser) for n·p > 30,
// p ≤ 0.5.
fn binomial_btpe(n: f32, p: f32, rng: ptr<function, Rng>) -> f32 {
    let r = p;
    let q = 1.0 - r;
    let fm = n * r + r;
    let m = floor(fm);
    let p1 = floor(2.195 * sqrt(n * r * q) - 4.6 * q) + 0.5;
    let xm = m + 0.5;
    let xl = xm - p1;
    let xr = xm + p1;
    let c = 0.134 + 20.5 / (15.3 + m);
    let a1 = (fm - xl) / (fm - xl * r);
    let laml = a1 * (1.0 + a1 / 2.0);
    let a2 = (xr - fm) / (xr * q);
    let lamr = a2 * (1.0 + a2 / 2.0);
    let p2 = p1 * (1.0 + 2.0 * c);
    let p3 = p2 + c / laml;
    let p4 = p3 + c / lamr;
    let nrq = n * r * q;
    loop {
        let u = uniform01(rng) * p4;
        var v = uniform01(rng);
        var y: f32;
        if u <= p1 {
            return floor(xm - p1 * v + u);
        } else if u <= p2 {
            let x = xl + (u - p1) / c;
            v = v * c + 1.0 - abs(m - x + 0.5) / p1;
            if v > 1.0 {
                continue;
            }
            y = floor(x);
        } else if u <= p3 {
            y = floor(xl + log(v) / laml);
            if y < 0.0 {
                continue;
            }
            v *= (u - p2) * laml;
        } else {
            y = floor(xr - log(v) / lamr);
            if y > n {
                continue;
            }
            v *= (u - p3) * lamr;
        }
        let k = abs(y - m);
        if k <= 20.0 || k >= nrq / 2.0 - 1.0 {
            let s = r / q;
            let a = s * (n + 1.0);
            var f = 1.0;
            if m < y {
                for (var i = m + 1.0; i <= y; i += 1.0) {
                    f *= a / i - s;
                }
            } else if m > y {
                for (var i = y + 1.0; i <= m; i += 1.0) {
                    f /= a / i - s;
                }
            }
            if v > f {
                continue;
            }
            return y;
        }
        let rho = (k / nrq) * ((k * (k / 3.0 + 0.625) + 0.16666666666666666) / nrq + 0.5);
        let t = -k * k / (2.0 * nrq);
        let a_log = log(v);
        if a_log < t - rho {
            return y;
        }
        if a_log > t + rho {
            continue;
        }
        let x1 = y + 1.0;
        let f1 = m + 1.0;
        let z = n - m + 1.0;
        let w = n - y + 1.0;
        let bound = xm * log(f1 / x1) + (n - m + 0.5) * log(z / w) + (y - m) * log(w * r / (x1 * q))
            + stirling_correction(f1) + stirling_correction(z) + stirling_correction(x1) + stirling_correction(w);
        if a_log > bound {
            continue;
        }
        return y;
    }
    return 0.0;
}

// numpy `rk_binomial`: work on the smaller of p and 1 − p.
fn sample_binomial(n: f32, p: f32, rng: ptr<function, Rng>) -> f32 {
    if n <= 0.0 {
        return 0.0;
    }
    if p <= 0.5 {
        if p * n <= 30.0 {
            return binomial_inversion(n, p, rng);
        }
        return binomial_btpe(n, p, rng);
    }
    let q = 1.0 - p;
    if q * n <= 30.0 {
        return n - binomial_inversion(n, q, rng);
    }
    return n - binomial_btpe(n, q, rng);
}

// Upstream `fast_interp`: the end samples outside the curve, else a binary
// search for the last sample at or below `x` (`searchsorted(side='right')`).
fn layer_density(x: f32, ch: u32) -> f32 {
    let k = params.n_samples;
    let layers_base = 6u * k;
    let stride = params.n_layers * 3u;
    let off = params.layer * 3u + ch;
    if x <= curves[ch] {
        return curves[layers_base + off];
    }
    if x >= curves[(k - 1u) * 3u + ch] {
        return curves[layers_base + (k - 1u) * stride + off];
    }
    var lo = 0u;
    var hi = k - 1u;
    while hi - lo > 1u {
        let mid = (lo + hi) / 2u;
        if curves[mid * 3u + ch] <= x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let t = (x - curves[lo * 3u + ch]) * curves[3u * k + lo * 3u + ch];
    let y0 = curves[layers_base + lo * stride + off];
    let y1 = curves[layers_base + (lo + 1u) * stride + off];
    return y0 + t * (y1 - y0);
}

// 256, not 1024 like the other per-pixel passes: the rejection samplers
// need enough registers that Metal will not run 1024-thread groups of this
// pipeline, and an oversized dispatch is dropped without an error. The host
// may lower it further on other GPUs (see `WgpuBackend::workgroup_size`).
override WORKGROUP_SIZE: u32 = 256u;

@compute @workgroup_size(WORKGROUP_SIZE)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    // The host spreads the dispatch over y once x would exceed the
    // 65535-workgroup limit (256-thread groups reach it at ~16.7 MP).
    let idx = gid.y * groups.x * WORKGROUP_SIZE + gid.x;
    if idx >= params.n_pixels { return; }
    let base = idx * 3u;
    let sign = select(1.0, -1.0, params.positive != 0u);

    for (var ch: u32 = 0u; ch < 3u; ch++) {
        let dmax = params.density_max[ch];
        let npp = params.n_particles_per_pixel[ch];
        let gu = params.grain_uniformity[ch];

        let d_in = layer_density(sign * density_cmy[base + ch], ch) + params.density_min[ch];
        let p = clamp(d_in / dmax, 1e-6, 1.0 - 1e-6);
        var value: f32;
        if npp <= 0.0 {
            // Noise-free limit: infinitely many grains develop the mean.
            value = p * dmax;
        } else {
            let od_particle = dmax / npp;
            let saturation = 1.0 - p * gu * (1.0 - 1e-6);
            let lambda = npp / saturation;
            // The CPU's stream split, seed = ch + 10·sub-layer, mixed with the
            // pixel index; SplitMix decorrelates the small integers before they
            // are combined. Monochrome (B&W) shares channel 0's stream.
            var seed_ch = ch;
            if params.monochrome != 0u {
                seed_ch = 0u;
            }
            let layer_seed = seed_ch + params.layer * 10u + params.base_seed;
            var rng = Rng(splitmix32(layer_seed) ^ splitmix32(idx), (splitmix32(idx ^ 0x9e3779b9u) << 1u) | 1u);
            let n_seeds = sample_poisson(lambda, &rng);
            let developed = sample_binomial(n_seeds, p, &rng);
            value = developed * od_particle * saturation;
        }
        if params.accumulate != 0u {
            out[base + ch] = out[base + ch] + value;
        } else {
            out[base + ch] = value;
        }
    }
}
