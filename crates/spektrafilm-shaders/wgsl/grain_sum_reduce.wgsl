// Per-channel sums of two RGB images (the unsharp mask's mass before and
// after), reduced to per-workgroup partial sums using exact 64-bit fixed-point
// integer accumulation (scale = 2^16, stored as bitcast f32 lo/hi u32 pairs
// for intermediate levels, converted to 6 f32s on the final level).
//
// Integer addition is strictly associative and commutative on every GPU
// (immune to -ffast-math on Metal/Vulkan/WebGPU), so whole-frame and tiled
// reductions produce the exact same bit-for-bit f32 sums regardless of tile
// grid or workgroup order.
//
// Mode 0: read RGB images `a` and `b` (2048 pixels/wg), write 12 u32s/block:
//   [a_lo.xyz, b_lo.xyz, a_hi.xyz, b_hi.xyz] (stored via bitcast<f32>).
// Mode 1: reduce previous level's 12-u32 entries -> 12 u32s/block.
// Mode 2: same as Mode 1 (or Mode 0 when `single_pass != 0`), but if
//   `final_to_f32 != 0` writes 6 f32s [Σa.xyz, Σb.xyz] so `grain_finalize.wgsl`
//   reads it directly; otherwise keeps the exact 12 u32s for Pass 1 readback.

struct Params {
    n_values: u32,
    mode: u32,          // 0 = pixels -> u64, 1 = u64 -> u64
    tile_width: u32,
    crop_x: u32,
    crop_y: u32,
    crop_w: u32,
    final_to_f32: u32,  // 1 = convert final block to 6 f32s; 0 = keep 12 u32s
    _pad1: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;

var<workgroup> pa_lo: array<vec3<u32>, 256>;
var<workgroup> pa_hi: array<vec3<u32>, 256>;
var<workgroup> pb_lo: array<vec3<u32>, 256>;
var<workgroup> pb_hi: array<vec3<u32>, 256>;

// Fixed-point scale: 2^16 = 65536. Optical densities are in [0, ~10], so each
// sample fits in u32 (< 65535 * 65536 < 2^32) and 100M pixels fit in ~46 bits.
const FP_SCALE: f32 = 65536.0;
const FP_INV: f32 = 1.0 / 65536.0;

fn to_fixed(v: vec3<f32>) -> vec3<u32> {
    let c = clamp(v, vec3<f32>(0.0), vec3<f32>(65535.0));
    return vec3<u32>(round(c * FP_SCALE));
}

fn add64_u32(a_lo: vec3<u32>, a_hi: vec3<u32>, b_lo: vec3<u32>, b_hi: vec3<u32>) -> array<vec3<u32>, 2> {
    let lo = a_lo + b_lo;
    let carry = vec3<u32>(
        select(0u, 1u, lo.x < a_lo.x),
        select(0u, 1u, lo.y < a_lo.y),
        select(0u, 1u, lo.z < a_lo.z),
    );
    let hi = a_hi + b_hi + carry;
    return array<vec3<u32>, 2>(lo, hi);
}

@compute @workgroup_size(256)
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let local = lid.x;
    let block = wid.y * groups.x + wid.x;
    let first = block * 2048u + local * 8u;
    var sa_lo = vec3<u32>(0u);
    var sa_hi = vec3<u32>(0u);
    var sb_lo = vec3<u32>(0u);
    var sb_hi = vec3<u32>(0u);
    for (var i = 0u; i < 8u; i++) {
        let j = first + i;
        if j < params.n_values {
            if params.mode == 0u {
                var px = j;
                if params.crop_w != 0u {
                    let ly = params.crop_y + j / params.crop_w;
                    let lx = params.crop_x + j % params.crop_w;
                    px = ly * params.tile_width + lx;
                }
                let va = to_fixed(vec3<f32>(a[3u * px], a[3u * px + 1u], a[3u * px + 2u]));
                let vb = to_fixed(vec3<f32>(b[3u * px], b[3u * px + 1u], b[3u * px + 2u]));
                let ra = add64_u32(sa_lo, sa_hi, va, vec3<u32>(0u));
                sa_lo = ra[0];
                sa_hi = ra[1];
                let rb = add64_u32(sb_lo, sb_hi, vb, vec3<u32>(0u));
                sb_lo = rb[0];
                sb_hi = rb[1];
            } else {
                let off = 12u * j;
                let va_lo = vec3<u32>(bitcast<u32>(a[off]), bitcast<u32>(a[off + 1u]), bitcast<u32>(a[off + 2u]));
                let vb_lo = vec3<u32>(bitcast<u32>(a[off + 3u]), bitcast<u32>(a[off + 4u]), bitcast<u32>(a[off + 5u]));
                let va_hi = vec3<u32>(bitcast<u32>(a[off + 6u]), bitcast<u32>(a[off + 7u]), bitcast<u32>(a[off + 8u]));
                let vb_hi = vec3<u32>(bitcast<u32>(a[off + 9u]), bitcast<u32>(a[off + 10u]), bitcast<u32>(a[off + 11u]));
                let ra = add64_u32(sa_lo, sa_hi, va_lo, va_hi);
                sa_lo = ra[0];
                sa_hi = ra[1];
                let rb = add64_u32(sb_lo, sb_hi, vb_lo, vb_hi);
                sb_lo = rb[0];
                sb_hi = rb[1];
            }
        }
    }
    pa_lo[local] = sa_lo;
    pa_hi[local] = sa_hi;
    pb_lo[local] = sb_lo;
    pb_hi[local] = sb_hi;
    workgroupBarrier();

    var stride = 128u;
    loop {
        if local < stride {
            let ra = add64_u32(pa_lo[local], pa_hi[local], pa_lo[local + stride], pa_hi[local + stride]);
            pa_lo[local] = ra[0];
            pa_hi[local] = ra[1];
            let rb = add64_u32(pb_lo[local], pb_hi[local], pb_lo[local + stride], pb_hi[local + stride]);
            pb_lo[local] = rb[0];
            pb_hi[local] = rb[1];
        }
        workgroupBarrier();
        if stride == 1u {
            break;
        }
        stride = stride / 2u;
    }

    if local == 0u && block * 2048u < params.n_values {
        if params.final_to_f32 != 0u {
            let o = 6u * block;
            let fa = (vec3<f32>(pa_hi[0]) * 4294967296.0 + vec3<f32>(pa_lo[0])) * FP_INV;
            let fb = (vec3<f32>(pb_hi[0]) * 4294967296.0 + vec3<f32>(pb_lo[0])) * FP_INV;
            output[o] = fa.x;
            output[o + 1u] = fa.y;
            output[o + 2u] = fa.z;
            output[o + 3u] = fb.x;
            output[o + 4u] = fb.y;
            output[o + 5u] = fb.z;
        } else {
            let o = 12u * block;
            output[o] = bitcast<f32>(pa_lo[0].x);
            output[o + 1u] = bitcast<f32>(pa_lo[0].y);
            output[o + 2u] = bitcast<f32>(pa_lo[0].z);
            output[o + 3u] = bitcast<f32>(pb_lo[0].x);
            output[o + 4u] = bitcast<f32>(pb_lo[0].y);
            output[o + 5u] = bitcast<f32>(pb_lo[0].z);
            output[o + 6u] = bitcast<f32>(pa_hi[0].x);
            output[o + 7u] = bitcast<f32>(pa_hi[0].y);
            output[o + 8u] = bitcast<f32>(pa_hi[0].z);
            output[o + 9u] = bitcast<f32>(pb_hi[0].x);
            output[o + 10u] = bitcast<f32>(pb_hi[0].y);
            output[o + 11u] = bitcast<f32>(pb_hi[0].z);
        }
    }
}
