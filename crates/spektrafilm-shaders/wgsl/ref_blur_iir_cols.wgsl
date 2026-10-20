// Column (vertical) direction of RefBlur's Young–van Vliet recursive
// Gaussian on an interleaved RGB buffer; the rows run in
// ref_blur_iir_rows.wgsl.
//
// CPU equivalent: `iir_blur_1d` / `iir_filter_row` in spektrafilm-math
// (σ ≥ 3): third-order forward then backward recursion, state initialized to
// the first sample of each direction. The CPU runs the direct form in f64;
// here the same filter runs as a first-order section followed by a
// second-order section in difference form, which is well conditioned in f32
// (the direct form's rounding error grows like 1/(1 − Σb), see `IirCascade`).
// The host computes g1, g2, c2 in f64.
//
// One invocation filters one channel of one column (three per column: the
// recursion is serial along the column, so more columns in flight keeps the
// GPU busy; 7.5 instead of 12.5 ms per pass at 24 MP). Neighbouring
// invocations touch neighbouring addresses. mid is filtered in place; the
// backward pass writes scale * result to output, or adds it when
// accumulate = 1. Only channels set in `mask` are touched.

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

fn sample_index(line: u32, i: u32, c: u32) -> u32 {
    if params.vertical != 0u {
        return (i * params.width + line) * 3u + c;
    }
    return (line * params.width + i) * 3u + c;
}

// Threads per workgroup; the host may lower it to what the GPU's
// compiled pipeline supports (see `WgpuBackend::workgroup_size`).
override WORKGROUP_SIZE: u32 = 64u;

@compute @workgroup_size(WORKGROUP_SIZE)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let t = gid.y * groups.x * WORKGROUP_SIZE + gid.x;
    var n = params.width;
    var lines = params.height;
    if params.vertical != 0u {
        n = params.height;
        lines = params.width;
    }
    // Consecutive invocations: the three channels of a line, then the next
    // line (adjacent in memory for columns).
    let line = t / 3u;
    let c = t % 3u;
    if line >= lines || params.mask[c] == 0u {
        return;
    }

    let g1 = params.g1[c];
    let g2 = params.g2[c];
    let c2 = params.c2[c];

    // Forward pass. Rows read the source; columns refilter mid in place
    // (each sample is read before it is overwritten). State: u (first
    // section), y and d = y − y₁ (second section), all starting at the
    // first sample like the CPU's w₁ = w₂ = w₃ = x₀.
    var x0: f32;
    if params.vertical != 0u {
        x0 = mid[sample_index(line, 0u, c)];
    } else {
        x0 = input[sample_index(line, 0u, c)];
    }
    var u = x0;
    var y = x0;
    var d = 0.0;
    for (var j = 0u; j < n; j++) {
        let k = sample_index(line, j, c);
        var s: f32;
        if params.vertical != 0u {
            s = mid[k];
        } else {
            s = input[k];
        }
        u = u + g1 * (s - u);
        d = g2 * (u - y) - c2 * d;
        y = y + d;
        mid[k] = y;
    }

    // Backward pass, starting from the last forward output.
    let xn = mid[sample_index(line, n - 1u, c)];
    u = xn;
    y = xn;
    d = 0.0;
    let scale = params.scale[c];
    for (var j = i32(n) - 1; j >= 0; j--) {
        let k = sample_index(line, u32(j), c);
        u = u + g1 * (mid[k] - u);
        d = g2 * (u - y) - c2 * d;
        y = y + d;
        if params.vertical == 0u {
            mid[k] = y;
        } else if params.accumulate != 0u {
            output[k] += scale * y;
        } else {
            output[k] = scale * y;
        }
    }
}
