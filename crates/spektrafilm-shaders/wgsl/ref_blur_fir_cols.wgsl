// One direction of a separable FIR Gaussian on an interleaved RGB buffer,
// all three channels per invocation, each with its own kernel: this file
// the vertical, ref_blur_fir_rows.wgsl the horizontal one.
//
// CPU equivalent: `fir_blur_1d` in spektrafilm-math (σ < 3), which uses
// `radius = int(3σ + 0.5)` and clamp-to-edge borders. The host builds the
// normalized kernels in f64 and stores them as three rows of 2·radius + 1
// taps, each centred and zero-padded to the largest radius. A channel with
// a smaller radius therefore first adds exact zeros, then the same products
// in the same order as its own kernel would. The vertical pass can scale its
// result and add it to the output, so blurs feeding a weighted sum need no
// extra buffer. Channels with mask 0 are not written.
//
// A workgroup first copies the pixels its taps read into `tile`, one vec4
// per pixel: a block of 16 columns × 32 rows plus the borders (two rows per
// invocation). Read from the buffer for every tap instead, a pass cost about
// 1 ms per tap at 24 MP.

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
const BLOCK_W: u32 = 16u;
const BLOCK_H: u32 = 32u;

// BLOCK_H + 2·MAX_RADIUS rows of BLOCK_W pixels.
var<workgroup> tile: array<vec4<f32>, 800>;
var<workgroup> weights: array<vec3<f32>, 19>; // 2·MAX_RADIUS + 1 taps

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
    @builtin(local_invocation_index) lid: u32,
) {
    let r = params.radius;
    let taps = 2u * r + 1u;
    if lid < taps {
        weights[lid] = vec3<f32>(kernel_buf[lid], kernel_buf[taps + lid], kernel_buf[2u * taps + lid]);
    }
    let w = params.width;
    let h = params.height;
    // Rows y0 − r … y0 + 31 + r, clamped, of columns x0 … x0 + 15.
    let x0 = wid.x * BLOCK_W;
    let y0 = wid.y * BLOCK_H;
    for (var e = lid; e < (BLOCK_H + 2u * r) * BLOCK_W; e += 256u) {
        let x = x0 + e % BLOCK_W;
        if x < w {
            let sy = clamp(i32(y0 + e / BLOCK_W) - i32(r), 0, i32(h) - 1);
            tile[e] = load_pixel(x, u32(sy));
        }
    }
    workgroupBarrier();
    let x = x0 + lid % BLOCK_W;
    if x < w {
        for (var half = 0u; half < 2u; half++) {
            let ty = lid / BLOCK_W + half * (BLOCK_H / 2u);
            if y0 + ty < h {
                var sum = vec3<f32>(0.0);
                for (var k = 0u; k < taps; k++) {
                    sum += weights[k] * tile[(ty + k) * BLOCK_W + lid % BLOCK_W].xyz;
                }
                store(x, y0 + ty, sum);
            }
        }
    }
}
