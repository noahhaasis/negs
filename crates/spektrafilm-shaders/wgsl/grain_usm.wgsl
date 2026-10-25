// Mass-conserving multiplicative unsharp mask, per-pixel parts.
//
// CPU equivalent: `multiplicative_unsharp_mask` in `spektrafilm_model::grain`
// (upstream `apply_multiplicative_unsharp_mask`, `model/diffusion.py`).
// Mode 0 clamps the density `d` at 0 before it is blurred into `g`; mode 1
// replaces `g` (the blur) with `D · (D / G)^amount`. The per-channel mass
// renormalization is `grain_sum_reduce.wgsl` + `grain_finalize.wgsl`.

struct Params {
    n_pixels: u32,
    mode: u32,
    amount: f32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> d: array<f32>;
@group(0) @binding(2) var<storage, read_write> g: array<f32>;

override WORKGROUP_SIZE: u32 = 1024u;

@compute @workgroup_size(WORKGROUP_SIZE)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let idx = gid.y * groups.x * WORKGROUP_SIZE + gid.x;
    if idx >= params.n_pixels { return; }
    let eps = 1e-6;
    for (var ch: u32 = 0u; ch < 3u; ch++) {
        let i = idx * 3u + ch;
        if params.mode == 0u {
            d[i] = max(d[i], 0.0);
        } else {
            let dv = d[i];
            if dv <= 0.0 {
                g[i] = 0.0;
            } else {
                g[i] = pow(max(dv, eps), 1.0 + params.amount) / pow(max(g[i], eps), params.amount);
            }
        }
    }
}
