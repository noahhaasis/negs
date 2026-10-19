// One direction of a Young–van Vliet recursive Gaussian on an interleaved RGB
// buffer. One invocation filters one whole row or column, all channels at once
// with per-channel coefficients; only channels set in `mask` are touched.
//
// CPU equivalent: `iir_blur_1d` / `iir_filter_row` in spektrafilm-math
// (σ ≥ 3): third-order forward then backward recursion, state initialized to
// the first sample of each direction. The host computes a, b1..b3 in f64.
//
// Rows (vertical = 0): input → mid, filtered in place in mid.
// Columns (vertical = 1): mid is filtered in place; the backward pass writes
// scale * result to output, or adds it when accumulate = 1.

struct Params {
    width: u32,
    height: u32,
    vertical: u32,
    accumulate: u32,
    mask: vec4<u32>, // 1 = channel filtered here
    scale: vec4<f32>,
    a: vec4<f32>,
    b1: vec4<f32>,
    b2: vec4<f32>,
    b3: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;        // [H*W*3]
@group(0) @binding(2) var<storage, read_write> mid: array<f32>;    // [H*W*3]
@group(0) @binding(3) var<storage, read_write> output: array<f32>; // [H*W*3]

fn pixel_index(line: u32, i: u32) -> u32 {
    if params.vertical != 0u {
        return (i * params.width + line) * 3u;
    }
    return (line * params.width + i) * 3u;
}

fn load_input(base: u32) -> vec3<f32> {
    return vec3<f32>(input[base], input[base + 1u], input[base + 2u]);
}

fn load_mid(base: u32) -> vec3<f32> {
    return vec3<f32>(mid[base], mid[base + 1u], mid[base + 2u]);
}

fn store_mid(base: u32, v: vec3<f32>) {
    for (var c = 0u; c < 3u; c++) {
        if params.mask[c] != 0u {
            mid[base + c] = v[c];
        }
    }
}

fn store_output(base: u32, v: vec3<f32>) {
    for (var c = 0u; c < 3u; c++) {
        if params.mask[c] != 0u {
            if params.accumulate != 0u {
                output[base + c] += params.scale[c] * v[c];
            } else {
                output[base + c] = params.scale[c] * v[c];
            }
        }
    }
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let line = gid.x;
    var n = params.width;
    var lines = params.height;
    if params.vertical != 0u {
        n = params.height;
        lines = params.width;
    }
    if line >= lines {
        return;
    }

    let a = params.a.xyz;
    let b1 = params.b1.xyz;
    let b2 = params.b2.xyz;
    let b3 = params.b3.xyz;

    // Forward pass. Rows read the source; columns refilter mid in place
    // (each sample is read before it is overwritten).
    var x0: vec3<f32>;
    if params.vertical != 0u {
        x0 = load_mid(pixel_index(line, 0u));
    } else {
        x0 = load_input(pixel_index(line, 0u));
    }
    var w1 = x0;
    var w2 = x0;
    var w3 = x0;
    for (var j = 0u; j < n; j++) {
        let base = pixel_index(line, j);
        var s: vec3<f32>;
        if params.vertical != 0u {
            s = load_mid(base);
        } else {
            s = load_input(base);
        }
        let w = a * s + b1 * w1 + b2 * w2 + b3 * w3;
        store_mid(base, w);
        w3 = w2;
        w2 = w1;
        w1 = w;
    }

    // Backward pass.
    let xn = load_mid(pixel_index(line, n - 1u));
    w1 = xn;
    w2 = xn;
    w3 = xn;
    for (var j = i32(n) - 1; j >= 0; j--) {
        let base = pixel_index(line, u32(j));
        let w = a * load_mid(base) + b1 * w1 + b2 * w2 + b3 * w3;
        if params.vertical != 0u {
            store_output(base, w);
        } else {
            store_mid(base, w);
        }
        w3 = w2;
        w2 = w1;
        w1 = w;
    }
}
