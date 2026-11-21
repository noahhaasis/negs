// The convert-film front pass (ticket 33): a scanned colour negative's
// linear RGB, read back in place to the film CMY density that would have
// scanned as it. The same solve as `FilmConverter::invert` in
// spektrafilm-core (upstream's `FilmConverter._invert`), in f32 as
// upstream's default precision:
//
//   y    = pretransform · rgb                     (calibration × gain)
//   x0   = clamp(cmy0 + ginv · (logrgb0 − log10 y), 0, x_hi)
//   f(x) = Σ_λ weight(λ) · 10^−(base(λ) + x · dye(λ))
//   J    = ∂f/∂x = −ln10 · Σ_λ weight(λ) · 10^−D(λ) · dye(λ)ᵀ
//   x   ← clamp(x − (JᵀJ + λI)⁻¹ Jᵀ (f − y), 0, x_hi)
//
// for at most 8 steps, stopping once every channel of f − y is under 1e-6.
//
// `n_pixels` is the number of pixels; each thread converts one.

struct Params {
    n_pixels: u32,
    n_wavelengths: u32,
    _pad0: u32,
    _pad1: u32,
    // Rows of the pretransform and of the seed's inverse gain matrix.
    pre0: vec4<f32>,
    pre1: vec4<f32>,
    pre2: vec4<f32>,
    ginv0: vec4<f32>,
    ginv1: vec4<f32>,
    ginv2: vec4<f32>,
    cmy0: vec4<f32>,
    logrgb0: vec4<f32>,
    x_hi: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
// Two per kept wavelength: (cyan, magenta, yellow dye density, base), then
// (the light's weight in r, g, b, 0).
@group(0) @binding(1) var<storage, read> spectra: array<vec4<f32>>;
// Linear scan RGB in, film density out.
@group(0) @binding(2) var<storage, read_write> data: array<f32>;

// Threads per workgroup; the host may lower it to what the GPU's
// compiled pipeline supports (see `WgpuBackend::workgroup_size`).
override WORKGROUP_SIZE: u32 = 1024u;

const LN10: f32 = 2.302585092994046;
const MAX_ITER: u32 = 8u;
const TOL: f32 = 1e-6;
const LAMBDA: f32 = 1e-7;

fn finite3(v: vec3<f32>) -> bool {
    // NaN fails both comparisons; infinities fail the bound.
    return all(abs(v) <= vec3<f32>(3.0e38));
}

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
    let i = idx * 3u;
    let rgb = vec3<f32>(data[i], data[i + 1u], data[i + 2u]);
    let y = vec3<f32>(dot(params.pre0.xyz, rgb), dot(params.pre1.xyz, rgb), dot(params.pre2.xyz, rgb));
    let x_hi = params.x_hi.xyz;

    // The analytic log-linear seed.
    let d = params.logrgb0.xyz - log(max(y, vec3<f32>(1e-12))) / LN10;
    var x = clamp(
        params.cmy0.xyz + vec3<f32>(dot(params.ginv0.xyz, d), dot(params.ginv1.xyz, d), dot(params.ginv2.xyz, d)),
        vec3<f32>(0.0),
        x_hi,
    );

    for (var iter = 0u; iter < MAX_ITER; iter++) {
        // The forward model and its Jacobian's columns (∂f/∂cyan, ∂magenta, ∂yellow).
        var f = vec3<f32>(0.0);
        var jc = vec3<f32>(0.0);
        var jm = vec3<f32>(0.0);
        var jy = vec3<f32>(0.0);
        for (var w = 0u; w < params.n_wavelengths; w++) {
            let dye_base = spectra[2u * w];
            let weight = spectra[2u * w + 1u].xyz;
            var light = exp(-LN10 * (dye_base.w + dot(x, dye_base.xyz)));
            // upstream's nan_to_num: no light where the density isn't a number.
            if !(light >= 0.0) {
                light = 0.0;
            }
            let l = weight * light;
            f += l;
            jc += l * dye_base.x;
            jm += l * dye_base.y;
            jy += l * dye_base.z;
        }
        let r = f - y;
        if all(abs(r) < vec3<f32>(TOL)) || !finite3(r) {
            break;
        }
        let jac = mat3x3<f32>(-LN10 * jc, -LN10 * jm, -LN10 * jy);
        let jt = transpose(jac);
        let a = jt * jac + mat3x3<f32>(LAMBDA, 0.0, 0.0, 0.0, LAMBDA, 0.0, 0.0, 0.0, LAMBDA);
        let g = jt * r;
        // (JᵀJ + λI)⁻¹ g by the adjugate; the matrix is symmetric.
        let c0 = cross(a[1], a[2]);
        let c1 = cross(a[2], a[0]);
        let c2 = cross(a[0], a[1]);
        let det = dot(a[0], c0);
        if !(abs(det) > 0.0) || !(abs(det) <= 3.0e38) {
            break;
        }
        let dx = vec3<f32>(dot(c0, g), dot(c1, g), dot(c2, g)) / det;
        if !finite3(dx) {
            break;
        }
        x = clamp(x - dx, vec3<f32>(0.0), x_hi);
    }

    data[i] = x.x;
    data[i + 1u] = x.y;
    data[i + 2u] = x.z;
}
