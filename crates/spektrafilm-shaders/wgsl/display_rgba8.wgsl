// The scan as sRGB display bytes, packed RGBA8 (R in the low byte), as
// `preview::display_rgba` makes them for an sRGB output: clipped to [0, 1],
// adjusted by the Post settings if any, sRGB-encoded unless the chain
// already encoded it, rounded to 8 bits. NaN is black, as there.
// `display_post.wgsl` is appended.

struct Params {
    n_pixels: u32,
    encoded: u32,
    _pad: vec2<u32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> rgb: array<f32>;
@group(0) @binding(2) var<storage, read_write> rgba: array<u32>;
@group(0) @binding(3) var<uniform> post: Post;
@group(0) @binding(4) var<storage, read> post_tables: array<f32>;

fn display_clip(v: f32) -> f32 {
    // NaN by its bits: compilers may fold `v != v` away.
    let x = select(v, 0.0, (bitcast<u32>(v) & 0x7fffffffu) > 0x7f800000u);
    return clamp(x, 0.0, 1.0);
}

fn display_byte(v: f32) -> u32 {
    var x = v;
    if params.encoded == 0u {
        x = select(1.055 * pow(x, 1.0 / 2.4) - 0.055, 12.92 * x, x <= 0.0031308);
    }
    return u32(floor(x * 255.0 + 0.5));
}

// Threads per workgroup; the host may lower it to what the GPU's
// compiled pipeline supports (see `WgpuBackend::workgroup_size`).
override WORKGROUP_SIZE: u32 = 256u;

@compute @workgroup_size(WORKGROUP_SIZE)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    // 2D grid above 65535 workgroups (see `linear_dispatch_grid`).
    let idx = gid.y * groups.x * WORKGROUP_SIZE + gid.x;
    if idx >= params.n_pixels {
        return;
    }
    let base = idx * 3u;
    let c = post_rgb(vec3<f32>(display_clip(rgb[base]), display_clip(rgb[base + 1u]), display_clip(rgb[base + 2u])));
    rgba[idx] = display_byte(c.r) | (display_byte(c.g) << 8u) | (display_byte(c.b) << 16u) | 0xff000000u;
}
