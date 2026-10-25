// DIR coupler per-pixel matrix multiply.
//
// CPU equivalent: in `compute_exposure_correction`:
//   density_silver = positive ? (density_max - density_cmy) : density_cmy
//   donor = Langmuir(density_silver)  (negatives; linear for positives)
//   correction[m] = sum_c donor[c] * couplers_matrix[c][m]
//
// `couplers_matrix` is already pre-scaled by `amount` on the CPU side.
// `positive` is encoded as 0 (negative) or 1 (positive film) in params.

struct Params {
    n_pixels: u32,
    positive: u32,
    _pad0: u32,
    _pad1: u32,
    density_max: vec4<f32>, // .xyz used, .w padding
    // Row-major couplers matrix [3][3], packed as three vec4s
    // (.w padding to satisfy std140 alignment).
    m_row0: vec4<f32>,
    m_row1: vec4<f32>,
    m_row2: vec4<f32>,
    // Donor Langmuir `x (k + x_ref) / (k + x)` on the channels whose
    // `donor_saturating` is non-zero (all zero: the linear donor).
    donor_k: vec4<f32>,
    donor_ref: vec4<f32>,
    donor_saturating: vec4<u32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> density_cmy: array<f32>;
@group(0) @binding(2) var<storage, read_write> correction: array<f32>;

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

    var silver: vec3<f32> = vec3<f32>(
        density_cmy[base],
        density_cmy[base + 1u],
        density_cmy[base + 2u],
    );
    if params.positive != 0u {
        silver = params.density_max.xyz - silver;
    }
    let k = params.donor_k.xyz;
    let saturated = silver * (k + params.donor_ref.xyz) / (k + silver);
    silver = select(silver, saturated, params.donor_saturating.xyz != vec3<u32>(0u));

    // correction[m] = silver[0]*M[0][m] + silver[1]*M[1][m] + silver[2]*M[2][m]
    let c0 = silver.x * params.m_row0.x + silver.y * params.m_row1.x + silver.z * params.m_row2.x;
    let c1 = silver.x * params.m_row0.y + silver.y * params.m_row1.y + silver.z * params.m_row2.y;
    let c2 = silver.x * params.m_row0.z + silver.y * params.m_row1.z + silver.z * params.m_row2.z;

    correction[base] = c0;
    correction[base + 1u] = c1;
    correction[base + 2u] = c2;
}
