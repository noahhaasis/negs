// Grain's last pass: the unsharp mask's per-channel mass renormalization
// (when `use_ratio`) and the removal of the density floor, into the density
// buffer.
//
// CPU equivalent: the end of `multiplicative_unsharp_mask` and of
// `apply_grain`'s channel worker (upstream `_finalize_grain`).
// `sums` = (Σ before r, g, b, Σ after r, g, b), from `grain_sum_reduce.wgsl`.

struct Params {
    n_pixels: u32,
    use_ratio: u32,
    _pad: vec2<u32>,
    density_min: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read> sums: array<f32>;
@group(0) @binding(3) var<storage, read_write> dst: array<f32>;

override WORKGROUP_SIZE: u32 = 1024u;

@compute @workgroup_size(WORKGROUP_SIZE)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let idx = gid.y * groups.x * WORKGROUP_SIZE + gid.x;
    if idx >= params.n_pixels { return; }
    for (var ch: u32 = 0u; ch < 3u; ch++) {
        var ratio = 1.0;
        if params.use_ratio != 0u && sums[3u + ch] > 0.0 {
            ratio = sums[ch] / sums[3u + ch];
        }
        let i = idx * 3u + ch;
        dst[i] = src[i] * ratio - params.density_min[ch];
    }
}
