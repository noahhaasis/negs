// Scanner colour noise reduction, pass 2 of 3: the joint bilateral filter
// on the grid. Each cell's chroma becomes the mean of its window's, weighted
// by distance (a disc falling off in d², 0 at the radius) and by how close each cell's luma
// is to its own; the cell's luma is kept. GPU port of `bilateral` in
// `spektrafilm-gpu/src/chroma_denoise.rs`, summing in the same order.

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
@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst: array<f32>;

override WORKGROUP_SIZE: u32 = 256u;

@compute @workgroup_size(WORKGROUP_SIZE)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let idx = gid.y * groups.x * WORKGROUP_SIZE + gid.x;
    if idx >= params.n { return; }
    let gw = i32(params.gw);
    let gh = i32(params.gh);
    let x = i32(idx % params.gw);
    let y = i32(idx / params.gw);
    let r = i32(params.radius);
    let rr = params.radius_cells;
    let l = src[idx * 3u];
    var sw = 0.0;
    var sr = 0.0;
    var sb = 0.0;
    for (var dy = -r; dy <= r; dy++) {
        let ny = y + dy;
        if ny < 0 || ny >= gh { continue; }
        for (var dx = -r; dx <= r; dx++) {
            let nx = x + dx;
            if nx < 0 || nx >= gw { continue; }
            let ws = max(1.0 - f32(dx * dx + dy * dy) / (rr * rr), 0.0);
            if ws <= 0.0 { continue; }
            let j = u32(ny * gw + nx) * 3u;
            let dl = src[j] - l;
            let wt = ws * exp(-dl * dl * params.range_k);
            sw += wt;
            sr += wt * src[j + 1u];
            sb += wt * src[j + 2u];
        }
    }
    dst[idx * 3u] = l;
    dst[idx * 3u + 1u] = sr / sw;
    dst[idx * 3u + 2u] = sb / sw;
}
