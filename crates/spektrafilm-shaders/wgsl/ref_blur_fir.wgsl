// One direction of a separable FIR Gaussian on an interleaved RGB buffer,
// all three channels per invocation, each with its own kernel.
//
// CPU equivalent: `fir_blur_1d` in spektrafilm-math (σ < 3), which uses
// `radius = int(3σ + 0.5)` and clamp-to-edge borders. The host builds the
// normalized kernels in f64 and stores them as three rows of 2·radius + 1
// taps, each centred and zero-padded to the largest radius. A channel with
// a smaller radius therefore first adds exact zeros, then the same products
// in the same order as its own kernel would. The vertical pass can scale its
// result and add it to the output, so blurs feeding a weighted sum need no
// extra buffer. Channels with mask 0 are not written.

struct Params {
    width: u32,
    height: u32,
    vertical: u32,
    accumulate: u32, // 1: output += scale * blur, 0: output = scale * blur
    radius: u32,     // largest radius of the active channels
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    mask: vec4<u32>,
    scale: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;       // [H*W*3]
@group(0) @binding(2) var<storage, read> kernel_buf: array<f32>;  // [3][2*radius+1]
@group(0) @binding(3) var<storage, read_write> output: array<f32>; // [H*W*3]

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;
    if x >= params.width || y >= params.height {
        return;
    }
    let r = i32(params.radius);
    let taps = u32(2 * r + 1);
    var sum = vec3<f32>(0.0);
    for (var k = 0; k <= 2 * r; k++) {
        var sx = i32(x);
        var sy = i32(y);
        if params.vertical != 0u {
            sy = clamp(sy + k - r, 0, i32(params.height) - 1);
        } else {
            sx = clamp(sx + k - r, 0, i32(params.width) - 1);
        }
        let b = (u32(sy) * params.width + u32(sx)) * 3u;
        let v = vec3<f32>(input[b], input[b + 1u], input[b + 2u]);
        let w = vec3<f32>(kernel_buf[u32(k)], kernel_buf[taps + u32(k)], kernel_buf[2u * taps + u32(k)]);
        sum += w * v;
    }
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
