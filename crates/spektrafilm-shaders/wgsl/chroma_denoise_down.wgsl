// Scanner colour noise reduction, pass 1 of 3: block means of log luma and
// red/blue log chroma on the grid. GPU port of `block_means` in
// `spektrafilm-gpu/src/chroma_denoise.rs` (same order of sums).

struct Params {
    width: u32,
    height: u32,
    gw: u32,
    gh: u32,
    factor: u32,
    radius: u32,
    n: u32,
    radius_cells: f32,
    strength: f32,
    range_k: f32,
    upsample_k: f32,
    log_offset: f32,
    luma: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> img: array<f32>;
@group(0) @binding(2) var<storage, read_write> grid: array<f32>;

override WORKGROUP_SIZE: u32 = 256u;

fn log_lcc(px: vec3<f32>) -> vec3<f32> {
    let f = log2(max(px, vec3<f32>(0.0)) + vec3<f32>(params.log_offset));
    let l = params.luma.x * f.x + params.luma.y * f.y + params.luma.z * f.z;
    return vec3<f32>(l, f.x - l, f.z - l);
}

@compute @workgroup_size(WORKGROUP_SIZE)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let idx = gid.y * groups.x * WORKGROUP_SIZE + gid.x;
    if idx >= params.n { return; }
    let gx = idx % params.gw;
    let gy = idx / params.gw;
    let k = params.factor;
    let x0 = gx * k;
    let x1 = min(x0 + k, params.width);
    let y0 = gy * k;
    let y1 = min(y0 + k, params.height);
    var sum = vec3<f32>(0.0);
    for (var y = y0; y < y1; y++) {
        for (var x = x0; x < x1; x++) {
            let i = (y * params.width + x) * 3u;
            let v = log_lcc(vec3<f32>(img[i], img[i + 1u], img[i + 2u]));
            sum.x += v.x;
            sum.y += v.y;
            sum.z += v.z;
        }
    }
    let n = f32((y1 - y0) * (x1 - x0));
    grid[idx * 3u] = sum.x / n;
    grid[idx * 3u + 1u] = sum.y / n;
    grid[idx * 3u + 2u] = sum.z / n;
}
