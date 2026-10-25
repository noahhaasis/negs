// Per-channel sums of two RGB images (the unsharp mask's mass before and
// after), reduced to per-workgroup partial sums.
//
// Mode 0 reads the images `a` and `b` (2048 pixels per workgroup: 256 threads
// × 8); mode 1 reduces a previous level's partials (six sums per entry, a then
// b, 2048 entries per workgroup). The host repeats mode 1 until one entry is
// left, like `max_reduce.wgsl`.

struct Params {
    n_values: u32,
    mode: u32,
    _pad: vec2<u32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;

var<workgroup> partial_a: array<vec3<f32>, 256>;
var<workgroup> partial_b: array<vec3<f32>, 256>;

@compute @workgroup_size(256)
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let local = lid.x;
    // Blocks wrap into y above 65535 workgroups (see `linear_dispatch_grid`);
    // the grid can overshoot, and blocks past the input write nothing.
    let block = wid.y * groups.x + wid.x;
    let first = block * 2048u + local * 8u;
    var sa = vec3<f32>(0.0);
    var sb = vec3<f32>(0.0);
    for (var i = 0u; i < 8u; i++) {
        let j = first + i;
        if j < params.n_values {
            if params.mode == 0u {
                sa += vec3<f32>(a[3u * j], a[3u * j + 1u], a[3u * j + 2u]);
                sb += vec3<f32>(b[3u * j], b[3u * j + 1u], b[3u * j + 2u]);
            } else {
                sa += vec3<f32>(a[6u * j], a[6u * j + 1u], a[6u * j + 2u]);
                sb += vec3<f32>(a[6u * j + 3u], a[6u * j + 4u], a[6u * j + 5u]);
            }
        }
    }
    partial_a[local] = sa;
    partial_b[local] = sb;
    workgroupBarrier();

    var stride = 128u;
    loop {
        if local < stride {
            partial_a[local] += partial_a[local + stride];
            partial_b[local] += partial_b[local + stride];
        }
        workgroupBarrier();
        if stride == 1u {
            break;
        }
        stride = stride / 2u;
    }

    if local == 0u && block * 2048u < params.n_values {
        let o = 6u * block;
        output[o] = partial_a[0].x;
        output[o + 1u] = partial_a[0].y;
        output[o + 2u] = partial_a[0].z;
        output[o + 3u] = partial_b[0].x;
        output[o + 4u] = partial_b[0].y;
        output[o + 5u] = partial_b[0].z;
    }
}
