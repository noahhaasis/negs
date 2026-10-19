// One direction of a Young–van Vliet recursive Gaussian on an interleaved RGB
// buffer. One invocation filters one whole row or column, all channels at once
// with per-channel coefficients; only channels set in `mask` are touched.
//
// CPU equivalent: `iir_blur_1d` / `iir_filter_row` in spektrafilm-math
// (σ ≥ 3): third-order forward then backward recursion, state initialized to
// the first sample of each direction. The CPU runs the direct form in f64;
// here the same filter runs as a first-order section followed by a
// second-order section in difference form, which is well conditioned in f32
// (the direct form's rounding error grows like 1/(1 − Σb), see `IirCascade`).
// The host computes g1, g2, c2 in f64.
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
    g1: vec4<f32>,  // 1 − p (real pole p)
    g2: vec4<f32>,  // 1 − c1 − c2 (complex pair: z² − c1·z − c2)
    c2: vec4<f32>,
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

    let g1 = params.g1.xyz;
    let g2 = params.g2.xyz;
    let c2 = params.c2.xyz;

    // Forward pass. Rows read the source; columns refilter mid in place
    // (each sample is read before it is overwritten). State: u (first
    // section), y and d = y − y₁ (second section), all starting at the
    // first sample like the CPU's w₁ = w₂ = w₃ = x₀.
    var x0: vec3<f32>;
    if params.vertical != 0u {
        x0 = load_mid(pixel_index(line, 0u));
    } else {
        x0 = load_input(pixel_index(line, 0u));
    }
    var u = x0;
    var y = x0;
    var d = vec3<f32>(0.0);
    for (var j = 0u; j < n; j++) {
        let base = pixel_index(line, j);
        var s: vec3<f32>;
        if params.vertical != 0u {
            s = load_mid(base);
        } else {
            s = load_input(base);
        }
        u = u + g1 * (s - u);
        d = g2 * (u - y) - c2 * d;
        y = y + d;
        store_mid(base, y);
    }

    // Backward pass, starting from the last forward output.
    let xn = load_mid(pixel_index(line, n - 1u));
    u = xn;
    y = xn;
    d = vec3<f32>(0.0);
    for (var j = i32(n) - 1; j >= 0; j--) {
        let base = pixel_index(line, u32(j));
        u = u + g1 * (load_mid(base) - u);
        d = g2 * (u - y) - c2 * d;
        y = y + d;
        if params.vertical != 0u {
            store_output(base, y);
        } else {
            store_mid(base, y);
        }
    }
}
