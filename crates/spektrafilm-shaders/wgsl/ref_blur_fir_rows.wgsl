// Horizontal direction of a separable FIR Gaussian on an interleaved RGB
// buffer, all three channels per invocation, each with its own kernel.
// ref_blur_fir_cols.wgsl is the vertical direction; see there for the
// kernels. Kept apart so that this pass reserves only the workgroup memory
// it needs, which lets more workgroups run at once.
//
// A workgroup first copies the pixels its taps read into `tile`, one vec4
// per pixel: a row segment of 256 pixels plus the borders. Read from the
// buffer for every tap instead, a pass cost about 1 ms per tap at 24 MP.

struct Params {
    width: u32,
    height: u32,
    accumulate: u32, // 1: output += scale * blur, 0: output = scale * blur
    radius: u32,     // largest radius of the active channels, ≤ MAX_RADIUS
    mask: vec4<u32>,
    scale: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;       // [H*W*3]
@group(0) @binding(2) var<storage, read> kernel_buf: array<f32>;  // [3][2*radius+1]
@group(0) @binding(3) var<storage, read_write> output: array<f32>; // [H*W*3]

const MAX_RADIUS: u32 = 9u; // int(3σ + 0.5) for σ < 3
const ROW_PIXELS: u32 = 256u;

var<workgroup> tile: array<vec4<f32>, 274>; // ROW_PIXELS + 2·MAX_RADIUS
var<workgroup> weights: array<vec3<f32>, 19>; // 2·MAX_RADIUS + 1 taps

// Python `_reflect` (scipy.ndimage `mode='reflect'`): d c b a | a b c d |
// d c b a, the edge sample repeated, with period 2n so that a kernel wider
// than the image keeps folding across it. Same as `reflect_index` in
// spektrafilm-math's gaussian.rs.
fn reflect_index(i: i32, n: i32) -> u32 {
    if i >= 0 && i < n {
        return u32(i);
    }
    // Fold the negative half first, then use unsigned remainder. Signed
    // remainder on negative coordinates is not portable across GPU drivers.
    let positive = select(i, -1 - i, i < 0);
    let period = u32(2 * n);
    let m = u32(positive) % period;
    return select(m, period - 1u - m, m >= u32(n));
}

fn load_pixel(x: u32, y: u32) -> vec4<f32> {
    let b = (y * params.width + x) * 3u;
    return vec4<f32>(input[b], input[b + 1u], input[b + 2u], 0.0);
}

fn store(x: u32, y: u32, sum: vec3<f32>) {
    let idx = (y * params.width + x) * 3u;
    for (var c = 0u; c < 3u; c++) {
        if params.mask[c] != 0u {
            if params.accumulate != 0u {
                output[idx + c] += params.scale[c] * sum[c];
            } else {
                output[idx + c] = params.scale[c] * sum[c];
            }
        }
    }
}

@compute @workgroup_size(256)
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {
    let r = params.radius;
    let taps = 2u * r + 1u;
    if lid < taps {
        weights[lid] = vec3<f32>(kernel_buf[lid], kernel_buf[taps + lid], kernel_buf[2u * taps + lid]);
    }
    let w = params.width;
    let h = params.height;
    // Pixels x0 − r … x0 + 255 + r of row y, reflected at the borders.
    // Segments are numbered row by row and wrap into y (see
    // `linear_dispatch_grid`), so images taller than 65535 rows fit.
    let segments = (w + ROW_PIXELS - 1u) / ROW_PIXELS;
    let segment = wid.y * groups.x + wid.x;
    if segment >= segments * h {
        return;
    }
    let x0 = (segment % segments) * ROW_PIXELS;
    let y = segment / segments;
    for (var p = lid; p < ROW_PIXELS + 2u * r; p += 256u) {
        tile[p] = load_pixel(reflect_index(i32(x0 + p) - i32(r), i32(w)), y);
    }
    workgroupBarrier();
    let x = x0 + lid;
    if x < w {
        var sum = vec3<f32>(0.0);
        for (var k = 0u; k < taps; k++) {
            sum += weights[k] * tile[lid + k].xyz;
        }
        store(x, y, sum);
    }
}
