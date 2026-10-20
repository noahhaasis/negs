// Row (horizontal) direction of RefBlur's recursive Gaussian (the cascade of
// ref_blur_iir_cols.wgsl), one invocation per channel of a row segment, so
// many more invocations run at once than one per row (48 → about 29 ms per
// row pass at 24 MP). The recursion is linear:
// over a segment of L samples the state S = (u, y, d) maps to
// S_L = A^L·S_0 + Z, with Z the segment's response from a zero state. So:
//
//   phase 0: every (row, segment, channel) runs its segment from a zero
//            state and stores Z;
//   phase 1: every (row, channel) walks its segments in order,
//            S_{k+1} = A^L·S_k + Z_k, starting from the CPU's initial state
//            (first sample, first sample, 0), and stores each S_k;
//   phase 2: every (row, segment, channel) reruns its segment from S_k and
//            writes the outputs.
//
// The forward pass reads `input` and writes `mid`; the backward pass
// (phases 3–5, the same over the segments from the end, starting from the
// last forward output) refilters `mid` in place. Exact in exact arithmetic;
// in f32 the combination step adds rounding of the order of the recursion's
// own (see `ref_blur_iir_matches_f64_recursion`). A^L comes from the host in
// f64 for the f32 coefficients the shader uses. Only channels set in `mask`
// are touched.

struct Params {
    width: u32,      // n, samples per row
    height: u32,     // rows
    segments: u32,   // K
    seg_len: u32,    // L; the last segment may be shorter
    mode: u32,       // 0–2 forward phases, 3–5 backward phases
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    mask: vec4<u32>,
    g1: vec4<f32>,
    g2: vec4<f32>,
    c2: vec4<f32>,
    // A^L per channel, rows (u, y, d) as vec4 (.w unused): a[3·c + row]
    a: array<vec4<f32>, 9>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;      // [H*W*3]
@group(0) @binding(2) var<storage, read_write> mid: array<f32>;  // [H*W*3]
@group(0) @binding(3) var<storage, read_write> state: array<f32>; // [H][K][3 ch][3]

fn state_index(row: u32, seg: u32, c: u32) -> u32 {
    return ((row * params.segments + seg) * 3u + c) * 3u;
}

// Position of the j-th sample of segment `seg` in processing order.
fn sample(row: u32, seg: u32, j: u32, c: u32, backward: bool) -> u32 {
    var i = seg * params.seg_len + j;
    if backward {
        i = params.width - 1u - i;
    }
    return (row * params.width + i) * 3u + c;
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
    let backward = params.mode >= 3u;
    let phase = params.mode % 3u;
    let c = t % 3u;
    let g1 = params.g1[c];
    let g2 = params.g2[c];
    let c2 = params.c2[c];

    if phase == 1u {
        let row = t / 3u;
        if row >= params.height || params.mask[c] == 0u {
            return;
        }
        var x0: f32;
        if backward {
            x0 = mid[(row * params.width + params.width - 1u) * 3u + c];
        } else {
            x0 = input[(row * params.width) * 3u + c];
        }
        var s = vec3<f32>(x0, x0, 0.0);
        let a0 = params.a[3u * c].xyz;
        let a1 = params.a[3u * c + 1u].xyz;
        let a2 = params.a[3u * c + 2u].xyz;
        for (var k = 0u; k < params.segments; k++) {
            let b = state_index(row, k, c);
            let z = vec3<f32>(state[b], state[b + 1u], state[b + 2u]);
            state[b] = s.x;
            state[b + 1u] = s.y;
            state[b + 2u] = s.z;
            s = vec3<f32>(dot(a0, s), dot(a1, s), dot(a2, s)) + z;
        }
        return;
    }

    let seg = (t / 3u) % params.segments;
    let row = t / (3u * params.segments);
    if row >= params.height || params.mask[c] == 0u {
        return;
    }
    let len = min(params.seg_len, params.width - seg * params.seg_len);
    var u = 0.0;
    var y = 0.0;
    var d = 0.0;
    let b = state_index(row, seg, c);
    if phase == 2u {
        u = state[b];
        y = state[b + 1u];
        d = state[b + 2u];
    }
    for (var j = 0u; j < len; j++) {
        let k = sample(row, seg, j, c, backward);
        var s: f32;
        if backward {
            s = mid[k];
        } else {
            s = input[k];
        }
        u = u + g1 * (s - u);
        d = g2 * (u - y) - c2 * d;
        y = y + d;
        if phase == 2u {
            mid[k] = y;
        }
    }
    if phase == 0u {
        state[b] = u;
        state[b + 1u] = y;
        state[b + 2u] = d;
    }
}
