// DIR coupler receiver saturation and exposure correction, positives only:
// `log_raw[m] -= S(correction[m])` with the receiver Langmuir
// `S(c) = c (k + c_ref) / (k + c)` on the channels whose `saturating` is
// non-zero. Takes the place of the plain `add_scaled` subtract.
//
// CPU equivalent: the receiver step and the subtraction at the end of
// `compute_exposure_correction`. `correction` is the diffused inhibitor.

struct Params {
    n_pixels: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    k: vec4<f32>,
    c_ref: vec4<f32>,
    saturating: vec4<u32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> correction: array<f32>;
@group(0) @binding(2) var<storage, read_write> log_raw: array<f32>;

// Threads per workgroup; the host may lower it to what the GPU's
// compiled pipeline supports (see `WgpuBackend::workgroup_size`).
override WORKGROUP_SIZE: u32 = 1024u;

@compute @workgroup_size(WORKGROUP_SIZE)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    // 2D grid above 65535 workgroups (see `linear_dispatch_grid`).
    let idx = gid.y * groups.x * WORKGROUP_SIZE + gid.x;
    if idx >= params.n_pixels { return; }
    let base = idx * 3u;

    let c = vec3<f32>(correction[base], correction[base + 1u], correction[base + 2u]);
    let k = params.k.xyz;
    let saturated = c * (k + params.c_ref.xyz) / (k + c);
    let arrived = select(c, saturated, params.saturating.xyz != vec3<u32>(0u));

    log_raw[base] -= arrived.x;
    log_raw[base + 1u] -= arrived.y;
    log_raw[base + 2u] -= arrived.z;
}
