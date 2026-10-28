// The spectral boundary guard, in place on the front pass's RGB input, as
// `adapted_xyz_to_tc_b` does on the CPU. Values outside the xy triangle are
// rewritten before `hanatos2025_rgb_to_raw`: X + Y + Z <= 0 or non-finite
// values become black (no exposure), and a negative X, Y or Z moves toward
// the adapted white onto the triangle at the same X + Y + Z, instead of
// folding x > 1 back inside. Values with no negative X, Y or Z are left
// untouched, so the front pass computes them exactly as without the guard.

struct Params {
    n_pixels: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    // RGB → CAT-adapted XYZ (the front pass's matrix) and its inverse.
    rgb_to_xyz: mat3x3<f32>,
    xyz_to_rgb: mat3x3<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> rgb: array<f32>;

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
    let xyz = params.rgb_to_xyz * vec3<f32>(rgb[base], rgb[base + 1u], rgb[base + 2u]);
    let b = xyz.x + xyz.y + xyz.z;
    let finite = abs(b) <= 3.0e38 && all(abs(xyz) <= vec3<f32>(3.0e38));
    var out: vec3<f32>;
    if !(b > 0.0) || !finite {
        out = vec3<f32>(0.0);
    } else if any(xyz < vec3<f32>(0.0)) {
        let white = params.rgb_to_xyz * vec3<f32>(1.0);
        let w = white / (white.x + white.y + white.z);
        let c = xyz / b;
        var t = 1.0;
        if c.x < 0.0 { t = min(t, w.x / (w.x - c.x)); }
        if c.y < 0.0 { t = min(t, w.y / (w.y - c.y)); }
        if c.z < 0.0 { t = min(t, w.z / (w.z - c.z)); }
        out = params.xyz_to_rgb * ((w + t * (c - w)) * b);
    } else {
        return;
    }
    rgb[base] = out.x;
    rgb[base + 1u] = out.y;
    rgb[base + 2u] = out.z;
}
