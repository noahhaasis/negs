// One direction of a separable FIR Gaussian on a single channel of an
// interleaved RGB buffer.
//
// CPU equivalent: `fir_blur_1d` in spektrafilm-math (σ < 3), which uses
// `radius = int(3σ + 0.5)` and clamp-to-edge borders. The host builds the
// normalized kernel in f64. The vertical pass can scale its result and add it
// to the output, so blurs feeding a weighted sum need no extra buffer.

struct Params {
    width: u32,
    height: u32,
    channel: u32,
    radius: u32,
    vertical: u32,
    accumulate: u32, // 1: output += scale * blur, 0: output = scale * blur
    scale: f32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;       // [H*W*3]
@group(0) @binding(2) var<storage, read> kernel_buf: array<f32>;  // [2*radius+1]
@group(0) @binding(3) var<storage, read_write> output: array<f32>; // [H*W*3]

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;
    if x >= params.width || y >= params.height {
        return;
    }

    let r = i32(params.radius);
    var sum: f32 = 0.0;
    for (var k = 0; k <= 2 * r; k++) {
        var sx = i32(x);
        var sy = i32(y);
        if params.vertical != 0u {
            sy = clamp(sy + k - r, 0, i32(params.height) - 1);
        } else {
            sx = clamp(sx + k - r, 0, i32(params.width) - 1);
        }
        sum += kernel_buf[k] * input[(u32(sy) * params.width + u32(sx)) * 3u + params.channel];
    }

    let idx = (y * params.width + x) * 3u + params.channel;
    if params.accumulate != 0u {
        output[idx] += params.scale * sum;
    } else {
        output[idx] = params.scale * sum;
    }
}
