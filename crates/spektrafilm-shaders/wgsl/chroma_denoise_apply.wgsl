// Scanner colour noise reduction, pass 3 of 3: joint bilateral upsampling.
// Each pixel's chroma moves toward the filtered chroma of the 3 × 3 cells
// around the nearest, weighted by a tent and by how close each cell's luma
// is to the pixel's; then the pixel is rescaled to its own linear
// luminance. In place on the scan RGB. GPU port of `apply` in
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
@group(0) @binding(1) var<storage, read> grid: array<f32>;
@group(0) @binding(2) var<storage, read_write> img: array<f32>;

override WORKGROUP_SIZE: u32 = 1024u;

// A pixel's position on the grid, in cells (cell centres at integers).
fn grid_position(x: u32) -> f32 {
    return (f32(x) + 0.5) / f32(params.factor) - 0.5;
}

// The upsampling's tent, reaching 1.5 cells: all 3 × 3 cells around the
// nearest count.
fn tent(d: f32) -> f32 {
    return max(1.0 - abs(d) / 1.5, 0.0);
}

@compute @workgroup_size(WORKGROUP_SIZE)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let idx = gid.y * groups.x * WORKGROUP_SIZE + gid.x;
    if idx >= params.n { return; }
    let u = grid_position(idx % params.width);
    let v = grid_position(idx / params.width);
    let cx = i32(round(u));
    let cy = i32(round(v));

    let w = params.luma.xyz;
    let b = idx * 3u;
    let px = vec3<f32>(img[b], img[b + 1u], img[b + 2u]);
    let base = max(px, vec3<f32>(0.0)) + vec3<f32>(params.log_offset);
    let f = log2(base);
    let l = w.x * f.x + w.y * f.y + w.z * f.z;
    var sw = 0.0;
    var sr = 0.0;
    var sb = 0.0;
    var fw = 0.0;
    var fr = 0.0;
    var fb = 0.0;
    for (var gy = cy - 1; gy <= cy + 1; gy++) {
        if gy < 0 || gy >= i32(params.gh) { continue; }
        let wy = tent(f32(gy) - v);
        for (var gx = cx - 1; gx <= cx + 1; gx++) {
            if gx < 0 || gx >= i32(params.gw) { continue; }
            let ws = wy * tent(f32(gx) - u);
            let j = (u32(gy) * params.gw + u32(gx)) * 3u;
            let dl = grid[j] - l;
            let wt = ws * exp(-dl * dl * params.upsample_k);
            sw += wt;
            sr += wt * grid[j + 1u];
            sb += wt * grid[j + 2u];
            fw += ws;
            fr += ws * grid[j + 1u];
            fb += ws * grid[j + 2u];
        }
    }
    // Far from all nine cells' luma (a lone speck): spatial weights only.
    var tr = fr / fw;
    var tb = fb / fw;
    if sw > 1e-20 {
        tr = sr / sw;
        tb = sb / sw;
    }
    let dr = params.strength * (tr - (f.x - l));
    let db = params.strength * (tb - (f.z - l));
    let dg = -(w.x * dr + w.z * db) / w.y;
    var out = px + base * (exp2(vec3<f32>(dr, dg, db)) - vec3<f32>(1.0));
    let y_in = dot(w, px);
    let y_out = dot(w, out);
    if y_in > 0.0 && y_out > 1e-8 {
        out = out * (y_in / y_out);
    }
    img[b] = out.x;
    img[b + 1u] = out.y;
    img[b + 2u] = out.z;
}
