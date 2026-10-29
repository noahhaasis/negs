// Element-wise passes around the FFT stages of the diffusion-filter
// convolution (see `FftConvolution` in wgpu_backend.rs). One thread per
// element of the n1 × n2 complex grid (or of the h × w output for mode 4).
//
// CPU equivalent: `convolve2d_reflect` in spektrafilm-math and the mix in
// `apply_diffusion_filter_um`:
//   out = (1 − p_s)·x + p_s·(reflect-pad(x, r) ⊛ psf)[r : r+h, r : r+w]
// The padded channel sits at the origin of the grid and the PSF is centred
// on (0, 0) with wrap-around, so the circular convolution of size
// n ≥ h + 2r equals the linear one on the output window.
//
// mode 0: grid = PSF of `channel` (unnormalized Σ a·exp(−r/λ) on the
//         (2r+1)² square around the origin, zero elsewhere).
// mode 1: kspec = Re(K) / (Re(K[0])·n1·n2): the PSF's spectrum normalized to
//         unit sum (K[0] is the PSF's sum) and to the unnormalized inverse.
// mode 2: grid = reflect-padded channel of `image`, zero outside.
// mode 3: grid *= kspec.
// mode 4: mix[c] = (1 − p_s)·image[c] + p_s·Re(grid[i + r, j + r]).
//
// Reduced (`k` > 1, for a device short of memory): the grid holds the image
// shrunk k times (h × w is then that size, `full_h` × `full_w` the image's),
// and the PSF is the one for that size.
// mode 5: as mode 2, of the k × k block means of `image`.
// mode 6: as mode 4 for each pixel of `image`, the blurred value sampled
//         bilinearly between the shrunk pixels' centres.

struct Params {
    mode: u32,
    n1: u32,       // grid rows (≥ h + 2r)
    n2: u32,       // grid columns (≥ w + 2r)
    h: u32,
    w: u32,
    r: u32,
    channel: u32,
    n_comp: u32,
    p_s: f32,
    k: u32,        // 1, or the reduction (modes 5 and 6)
    full_h: u32,
    full_w: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> grid: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read_write> kspec: array<f32>;
@group(0) @binding(3) var<storage, read> image: array<f32>;      // h·w·3
@group(0) @binding(4) var<storage, read_write> mix: array<f32>;  // h·w·3
@group(0) @binding(5) var<storage, read> comps: array<vec2<f32>>; // (λ_px, amplitude) per channel, 16 slots each

fn reflect(s: i32, n: i32) -> u32 {
    if s < 0 {
        return u32(-s);
    }
    if s >= n {
        return u32(2 * (n - 1) - s);
    }
    return u32(s);
}

// Threads per workgroup; the host may lower it to what the GPU's
// compiled pipeline supports (see `WgpuBackend::workgroup_size`).
override WORKGROUP_SIZE: u32 = 256u;

@compute @workgroup_size(WORKGROUP_SIZE)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let idx = gid.y * groups.x * WORKGROUP_SIZE + gid.x;
    let n1 = params.n1;
    let n2 = params.n2;
    let r = params.r;
    if params.mode == 6u {
        if idx >= params.full_h * params.full_w {
            return;
        }
        let i = idx / params.full_w;
        let j = idx % params.full_w;
        let kf = f32(params.k);
        let u = clamp((f32(i) + 0.5) / kf - 0.5, 0.0, f32(params.h - 1u));
        let v = clamp((f32(j) + 0.5) / kf - 0.5, 0.0, f32(params.w - 1u));
        let i0 = u32(u);
        let j0 = u32(v);
        let i1 = min(i0 + 1u, params.h - 1u);
        let j1 = min(j0 + 1u, params.w - 1u);
        let fu = u - f32(i0);
        let fv = v - f32(j0);
        // `mix` is the output here, not the builtin.
        let top = grid[(i0 + r) * n2 + j0 + r].x * (1.0 - fv) + grid[(i0 + r) * n2 + j1 + r].x * fv;
        let bottom = grid[(i1 + r) * n2 + j0 + r].x * (1.0 - fv) + grid[(i1 + r) * n2 + j1 + r].x * fv;
        let blurred = top * (1.0 - fu) + bottom * fu;
        let k = idx * 3u + params.channel;
        mix[k] = (1.0 - params.p_s) * image[k] + params.p_s * blurred;
        return;
    }
    if params.mode == 4u {
        if idx >= params.h * params.w {
            return;
        }
        let i = idx / params.w;
        let j = idx % params.w;
        let k = idx * 3u + params.channel;
        let blurred = grid[(i + r) * n2 + j + r].x;
        mix[k] = (1.0 - params.p_s) * image[k] + params.p_s * blurred;
        return;
    }
    if idx >= n1 * n2 {
        return;
    }
    let y = idx / n2;
    let x = idx % n2;
    if params.mode == 0u {
        var dy: i32 = -1;
        var dx: i32 = -1;
        if y <= r {
            dy = i32(y);
        } else if y >= n1 - r {
            dy = i32(n1 - y);
        }
        if x <= r {
            dx = i32(x);
        } else if x >= n2 - r {
            dx = i32(n2 - x);
        }
        var v = 0.0;
        if dy >= 0 && dx >= 0 {
            let rr = sqrt(f32(dx * dx + dy * dy));
            for (var k = 0u; k < params.n_comp; k++) {
                let c = comps[params.channel * 16u + k];
                v += c.y * exp(-rr / c.x);
            }
        }
        grid[idx] = vec2<f32>(v, 0.0);
    } else if params.mode == 1u {
        kspec[idx] = grid[idx].x / (grid[0].x * f32(n1) * f32(n2));
    } else if params.mode == 5u {
        var v = 0.0;
        if y < params.h + 2u * r && x < params.w + 2u * r {
            let sy = reflect(i32(y) - i32(r), i32(params.h));
            let sx = reflect(i32(x) - i32(r), i32(params.w));
            // The block's pixels within the image; the last row and column
            // of blocks may hold fewer.
            let y0 = sy * params.k;
            let x0 = sx * params.k;
            let y1 = min(y0 + params.k, params.full_h);
            let x1 = min(x0 + params.k, params.full_w);
            var sum = 0.0;
            for (var yy = y0; yy < y1; yy++) {
                for (var xx = x0; xx < x1; xx++) {
                    sum += image[(yy * params.full_w + xx) * 3u + params.channel];
                }
            }
            v = sum / f32((y1 - y0) * (x1 - x0));
        }
        grid[idx] = vec2<f32>(v, 0.0);
    } else if params.mode == 2u {
        var v = 0.0;
        if y < params.h + 2u * r && x < params.w + 2u * r {
            let sy = reflect(i32(y) - i32(r), i32(params.h));
            let sx = reflect(i32(x) - i32(r), i32(params.w));
            v = image[(sy * params.w + sx) * 3u + params.channel];
        }
        grid[idx] = vec2<f32>(v, 0.0);
    } else {
        grid[idx] = grid[idx] * kspec[idx];
    }
}
