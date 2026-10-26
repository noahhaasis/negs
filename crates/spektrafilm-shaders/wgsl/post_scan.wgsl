// Final resident-chain post-scan pass, in place, like the CPU scan's
// `encode_output`: Python's near-identity RGB_to_RGB matrix, added as
// `rgb + delta · rgb`, then the sRGB OETF, unclipped (linear below 0, the
// power law above 1). Displays and integer files clip afterwards.

struct Params {
    n_pixels: u32,
    output_cctf_encoding: u32,
    _pad: vec2<u32>,
    // Rows of (matrix - identity); w unused.
    delta: array<vec4<f32>, 3>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> rgb: array<f32>;

fn srgb_encode(x: f32) -> f32 {
    if x <= 0.0031308 {
        return 12.92 * x;
    }
    return 1.055 * pow(x, 1.0 / 2.4) - 0.055;
}

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
    if idx >= params.n_pixels {
        return;
    }
    let base = idx * 3u;
    if params.output_cctf_encoding != 0u {
        let c = vec3<f32>(rgb[base], rgb[base + 1u], rgb[base + 2u]);
        let r = c.x + dot(params.delta[0].xyz, c);
        let g = c.y + dot(params.delta[1].xyz, c);
        let b = c.z + dot(params.delta[2].xyz, c);
        rgb[base] = srgb_encode(r);
        rgb[base + 1u] = srgb_encode(g);
        rgb[base + 2u] = srgb_encode(b);
    }
}
