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
// Forward phase 0 first copies `input` to `mid`; from there phases 0 and 2
// read `mid` (phase 1 takes the first sample from `input`), and phase 2
// filters it in place. The backward pass
// (phases 3–5, the same over the segments from the end, starting from the
// last forward output) refilters `mid` in place. Exact in exact arithmetic;
// in f32 the combination step adds rounding of the order of the recursion's
// own (see `ref_blur_iir_matches_f64_recursion`). A^L comes from the host in
// f64 for the f32 coefficients the shader uses. Only channels set in `mask`
// are touched.
//
// Phases 0 and 2 run one workgroup per segment of BLOCK rows. It walks the
// segment CHUNK samples at a time: first it copies the next CHUNK samples
// of the segment in each of its rows (all three channels, contiguous in
// memory) into `tile`, then each invocation filters its channel of a row
// from there, and phase 2 copies the outputs back the same way. Reading
// straight from the buffer instead, the invocations of a SIMD group touch
// segments L samples apart, so every load instruction hits about 11 cache
// lines; the four phases took 29 ms at 24 MP, against 16 now. A chunk of
// 8 samples keeps `tile` small enough for many workgroups per core (16 and
// 32 were slower).

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

const BLOCK: u32 = 32u;  // rows per workgroup (one 32-wide Apple SIMD group per channel)
const CHUNK: u32 = 8u;   // samples staged at a time
const SPAN: u32 = 24u;   // floats per row and chunk: 3·CHUNK
const STEP: u32 = 4u;    // rows the 96 invocations stage at once: 96 / SPAN
// Floats per row in `tile`: SPAN plus one, so that the 32 rows a SIMD
// group reads sit in different banks.
const RUN: u32 = 25u;

var<workgroup> tile: array<f32, 800>; // BLOCK · RUN

// Fixed: the staging layout needs all 96 invocations. Apple GPUs run it
// (see `metal_threadgroup_limits_match_apple_gpus`).
// TODO(intel): check the pipeline limit on Intel/AMD GPUs is at least 96;
// below it the host only warns.
@compute @workgroup_size(96)
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {
    // Workgroups are numbered along x and wrap into y (see
    // `linear_dispatch_grid`).
    let group = wid.y * groups.x + wid.x;
    if params.mode % 3u == 1u {
        combine(group * 96u + lid);
    } else {
        // Segment-major: the segments of a block of rows, then the next.
        let blocks = (params.height + BLOCK - 1u) / BLOCK;
        if group >= params.segments * blocks {
            return;
        }
        run_segment(group % params.segments, (group / params.segments) * BLOCK, lid);
    }
}

// Phase 1: one invocation per (row, channel).
fn combine(t: u32) {
    let backward = params.mode >= 3u;
    let c = t % 3u;
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
}

// Phases 0 and 2 for segment `seg` of rows row0 .. row0 + BLOCK.
fn run_segment(seg: u32, row0: u32, lid: u32) {
    let backward = params.mode >= 3u;
    let phase = params.mode % 3u;
    let len = min(params.seg_len, params.width - seg * params.seg_len);
    let line = 3u * params.width;

    // Forward phase 0 first copies the segment's rows from the input to
    // mid, so that phases 0 and 2 read mid and phase 2 updates it in place.
    // Written a chunk at a time instead, mid's cache lines are filled
    // piecewise, which made writing them cost twice as much.
    if params.mode == 0u {
        let n = 3u * len;
        for (var rr = 0u; rr < BLOCK; rr++) {
            if row0 + rr < params.height {
                let first = sample(row0 + rr, seg, 0u, 0u, false);
                for (var e = lid; e < n; e += 96u) {
                    if params.mask[e % 3u] != 0u {
                        mid[first + e] = input[first + e];
                    }
                }
            }
        }
        storageBarrier();
    }

    // Staging: this invocation copies float `off` of the chunk for rows
    // lid / SPAN, + STEP, + 2·STEP, ...
    let off = lid % SPAN;
    let j_off = off / 3u;
    let c_off = off % 3u;
    let r_off = lid / SPAN;
    let copy = params.mask[c_off] != 0u;
    // Buffer index of float `off` of the first chunk in row row0 + r_off.
    var start = sample(row0 + r_off, seg, j_off, c_off, backward);

    // Filtering: one channel of one row.
    let c = lid / BLOCK;
    let r = lid % BLOCK;
    let row = row0 + r;
    let mine = row < params.height && params.mask[c] != 0u;
    let g1 = params.g1[c];
    let g2 = params.g2[c];
    let c2 = params.c2[c];
    var u = 0.0;
    var y = 0.0;
    var d = 0.0;
    let b = state_index(row, seg, c);
    if mine && phase == 2u {
        u = state[b];
        y = state[b + 1u];
        d = state[b + 2u];
    }

    let chunks = (len + CHUNK - 1u) / CHUNK;
    for (var k = 0u; k < chunks; k++) {
        let j0 = k * CHUNK;
        let staged = copy && j0 + j_off < len;
        var i = start;
        for (var rr = r_off; rr < BLOCK; rr += STEP) {
            if staged && row0 + rr < params.height {
                tile[rr * RUN + off] = mid[i];
            }
            i += STEP * line;
        }
        workgroupBarrier();
        if mine {
            let n = min(CHUNK, len - j0);
            let base = r * RUN + c;
            for (var j = 0u; j < n; j++) {
                let at = base + 3u * j;
                let s = tile[at];
                u = u + g1 * (s - u);
                d = g2 * (u - y) - c2 * d;
                y = y + d;
                if phase == 2u {
                    tile[at] = y;
                }
            }
        }
        workgroupBarrier();
        // Each invocation stores the entries it loaded, so the next chunk's
        // loads need no barrier before them.
        if phase == 2u {
            i = start;
            for (var rr = r_off; rr < BLOCK; rr += STEP) {
                if staged && row0 + rr < params.height {
                    mid[i] = tile[rr * RUN + off];
                }
                i += STEP * line;
            }
        }
        if backward {
            start -= 3u * CHUNK;
        } else {
            start += 3u * CHUNK;
        }
    }
    if mine && phase == 0u {
        state[b] = u;
        state[b + 1u] = y;
        state[b + 2u] = d;
    }
}
