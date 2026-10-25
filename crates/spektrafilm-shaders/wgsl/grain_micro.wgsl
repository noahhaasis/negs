// Grain micro-structure: multiplicative lognormal clumping with mean 1.
//
// CPU equivalent: `add_micro_structure` in `spektrafilm_model::grain`
// (upstream `model/grain.py`). Only on at very small pixels (clumps of at
// least 5 % of a pixel). Modes: 0 writes the clumping field to `noise` (it is
// blurred next), 1 multiplies `acc` by `noise`, 2 multiplies `acc` by fresh
// clumping in one pass (no blur).

struct Params {
    n_pixels: u32,
    base_seed: u32,
    mode: u32,
    monochrome: u32,
    // ln(1 + sigma²) of the lognormal.
    sigma2: f32,
    _pad: vec3<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> acc: array<f32>;
@group(0) @binding(2) var<storage, read_write> noise: array<f32>;

fn splitmix32(x: u32) -> u32 {
    var z: u32 = x;
    z = (z ^ (z >> 16u)) * 0x85ebca6bu;
    z = (z ^ (z >> 13u)) * 0xc2b2ae35u;
    return z ^ (z >> 16u);
}

fn hash_uniform(a: u32, b: u32) -> f32 {
    return (f32(splitmix32(a ^ splitmix32(b)) >> 8u) + 0.5) * (1.0 / 16777216.0);
}

// Box-Muller from two hashed uniforms of this pixel and channel.
fn clump(idx: u32, ch: u32) -> f32 {
    var lane = ch;
    if params.monochrome != 0u {
        lane = 0u;
    }
    let key = params.base_seed + 1000u + lane * 2u;
    let u1 = hash_uniform(idx, key);
    let u2 = hash_uniform(idx, key + 1u);
    let z = sqrt(-2.0 * log(u1)) * cos(6.283185307179586 * u2);
    return exp(z * sqrt(params.sigma2) - 0.5 * params.sigma2);
}

override WORKGROUP_SIZE: u32 = 1024u;

@compute @workgroup_size(WORKGROUP_SIZE)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let idx = gid.y * groups.x * WORKGROUP_SIZE + gid.x;
    if idx >= params.n_pixels { return; }
    for (var ch: u32 = 0u; ch < 3u; ch++) {
        let i = idx * 3u + ch;
        if params.mode == 0u {
            noise[i] = clump(idx, ch);
        } else if params.mode == 1u {
            acc[i] = acc[i] * noise[i];
        } else {
            acc[i] = acc[i] * clump(idx, ch);
        }
    }
}
