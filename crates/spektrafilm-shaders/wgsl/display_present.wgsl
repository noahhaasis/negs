// The canvas. The scan's display bytes, as `display_rgba8.wgsl` makes them,
// become the shown image (`make_shown`: a texture with mip levels), which a
// render pass draws where the page's view puts the photograph (`fs_view`).
// Pan and zoom draw it again without the film chain, and a Post edit makes
// it again from the same scan. Canvas and texture are 8-bit unorm, so
// byte / 255 stores the byte itself, and a pixel drawn 1:1 is the byte the
// scan made. `display_post.wgsl` is appended.

struct Bytes {
    width: u32,
    height: u32,
    encoded: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> bytes: Bytes;
@group(0) @binding(1) var<storage, read> rgb: array<f32>;
@group(0) @binding(2) var shown_out: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(7) var<uniform> post: Post;
@group(0) @binding(8) var<storage, read> post_tables: array<f32>;

fn display_clip(v: f32) -> f32 {
    // NaN by its bits: compilers may fold `v != v` away.
    let x = select(v, 0.0, (bitcast<u32>(v) & 0x7fffffffu) > 0x7f800000u);
    return clamp(x, 0.0, 1.0);
}

fn display_byte(v: f32) -> f32 {
    var x = v;
    if bytes.encoded == 0u {
        x = select(1.055 * pow(x, 1.0 / 2.4) - 0.055, 12.92 * x, x <= 0.0031308);
    }
    return floor(x * 255.0 + 0.5) / 255.0;
}

@compute @workgroup_size(16, 16)
fn make_shown(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= bytes.width || gid.y >= bytes.height {
        return;
    }
    let base = (gid.y * bytes.width + gid.x) * 3u;
    let c = post_rgb(vec3<f32>(display_clip(rgb[base]), display_clip(rgb[base + 1u]), display_clip(rgb[base + 2u])));
    textureStore(shown_out, gid.xy, vec4<f32>(display_byte(c.r), display_byte(c.g), display_byte(c.b), 1.0));
}

// Where the shown image lands on the canvas, in device pixels: its top-left
// corner and device pixels per texel. `exact` draws it 1:1 from a
// whole-pixel `origin`; otherwise it's sampled, trilinear below 1 (`lod`)
// and bilinear above, as the macOS canvas filters.
struct View {
    origin: vec2<f32>,
    scale: vec2<f32>,
    size: vec2<f32>,
    lod: f32,
    exact: u32,
    background: vec4<f32>,
}

@group(0) @binding(3) var<uniform> view: View;
@group(0) @binding(4) var shown: texture_2d<f32>;
@group(0) @binding(5) var bilinear: sampler;

// One triangle over the whole viewport.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_view(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let t = (pos.xy - view.origin) / view.scale;
    if any(t < vec2<f32>(0.0)) || any(t >= view.size) {
        return view.background;
    }
    if view.exact == 1u {
        return textureLoad(shown, vec2<i32>(t), 0);
    }
    return textureSampleLevel(shown, bilinear, t / view.size, view.lod);
}

// A mip level from the one above it: each pixel the average of the 2×2
// above it, one bilinear sample at their shared corner.
@group(0) @binding(6) var finer: texture_2d<f32>;

@fragment
fn fs_mip(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = max(vec2<u32>(1u), textureDimensions(finer) >> vec2<u32>(1u));
    return textureSampleLevel(finer, bilinear, pos.xy / vec2<f32>(size), 0.0);
}
