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
    border: Border,
    // Source size, output clipping (off while editing), padding. Affine
    // rows map normalized output coordinates back into the whole scan.
    source: vec4<f32>,
    map_x: vec4<f32>,
    map_y: vec4<f32>,
}

// A presentation border around the photograph (`BorderView` in
// wgpu_backend.rs, laid out by the page's web/border.js as
// spektrafilm-io's border.rs does). Rectangles are left, top, width,
// height in the photograph's own pixels, from its top-left corner. From the
// bottom up: `fill` over the canvas (Instagram's extension), `mat` over the
// bordered `frame`, `line_color` over the keyline's ring, the photograph,
// then the texture with its alpha.
struct Border {
    // The photograph's width and height; zero for no border.
    photo: vec4<f32>,
    canvas: vec4<f32>,
    frame: vec4<f32>,
    line: vec4<f32>,
    fill: vec4<f32>,
    mat: vec4<f32>,
    line_color: vec4<f32>,
    texture: vec4<f32>,
    // Kind (0 none, 1 the 120 frame's nine-patch, 2 the 35mm strip), turned
    // a quarter, and the nine-patch's corner size and canvas pixels per texel.
    texture_fit: vec4<f32>,
}

@group(0) @binding(3) var<uniform> view: View;
@group(0) @binding(4) var shown: texture_2d<f32>;
@group(0) @binding(5) var bilinear: sampler;
@group(0) @binding(9) var border_texture: texture_2d<f32>;

// One triangle over the whole viewport.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn source_at(t: vec2<f32>) -> vec2<f32> {
    let p = vec3<f32>(t / view.size, 1.0);
    return vec2<f32>(dot(view.map_x.xyz, p), dot(view.map_y.xyz, p));
}

fn photo_at(t: vec2<f32>) -> vec4<f32> {
    let uv = source_at(t);
    if view.exact == 1u {
        let pixel = clamp(vec2<i32>(floor(uv * view.source.xy)), vec2<i32>(0), vec2<i32>(view.source.xy) - vec2<i32>(1));
        return textureLoad(shown, pixel, 0);
    }
    return textureSampleLevel(shown, bilinear, uv, view.lod);
}

fn inside(p: vec2<f32>, r: vec4<f32>) -> bool {
    return all(p >= r.xy) && all(p < r.xy + r.zw);
}

// One axis of the nine-patch, as `axis` in border.rs: the first and last
// `pad` canvas pixels map to the texture's ends at `scale`, the middle
// stretches. A fraction of the texture's `size`.
fn patch_axis(c: f32, dest: f32, size: f32, scale: f32, pad: f32) -> f32 {
    let tex_pad = pad / scale;
    var src = tex_pad + (c - pad) * (size - 2.0 * tex_pad) / max(dest - 2.0 * pad, 1.0);
    if c < pad {
        src = c / scale;
    } else if c > dest - pad {
        src = size - (dest - c) / scale;
    }
    return src / size;
}

// The texture at `d`, photograph pixels into its rectangle, drawn at `s`
// device pixels per photograph pixel. A turned texture is laid out as
// border.rs lays it out: the 120 frame `rotate270`'d, the strip as
// `Renderer` turns it.
fn border_texel(d: vec2<f32>, s: f32) -> vec4<f32> {
    let dest = view.border.texture.zw;
    let dims = vec2<f32>(textureDimensions(border_texture));
    let turned = view.border.texture_fit.y > 0.5;
    let size = select(dims, dims.yx, turned);
    var n = d / dest;
    var texels = max(size.x / dest.x, size.y / dest.y);
    if view.border.texture_fit.x < 1.5 {
        let pad = view.border.texture_fit.z;
        let scale = view.border.texture_fit.w;
        n = vec2<f32>(patch_axis(d.x, dest.x, size.x, scale, pad), patch_axis(d.y, dest.y, size.y, scale, pad));
        texels = 1.0 / scale;
    }
    let uv = select(n, vec2<f32>(1.0 - n.y, n.x), turned);
    return textureSampleLevel(border_texture, bilinear, uv, max(0.0, log2(texels / s)));
}

@fragment
fn fs_view(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let t = (pos.xy - view.origin) / view.scale;
    let uv = source_at(t);
    let in_output = all(t >= vec2<f32>(0.0)) && all(t < view.size);
    let in_photo = all(uv >= vec2<f32>(0.0)) && all(uv < vec2<f32>(1.0)) && (view.source.z < 0.5 || in_output);
    let b = view.border;
    if b.photo.x == 0.0 {
        if !in_photo {
            return view.background;
        }
        return photo_at(t);
    }
    // Photograph pixels: at 100%, whole at the canvas's pixel edges.
    let p = t * b.photo.xy / view.size;
    if !inside(p, b.canvas) {
        return view.background;
    }
    var c = b.fill.rgb;
    if inside(p, b.frame) {
        c = b.mat.rgb;
    }
    if inside(p, b.line) {
        c = b.line_color.rgb;
    }
    if in_photo {
        c = photo_at(t).rgb;
    }
    if b.texture_fit.x > 0.0 && inside(p, b.texture) {
        let s = view.scale.x * view.size.x / b.photo.x;
        let tex = border_texel(p - b.texture.xy, s);
        c = mix(c, tex.rgb, tex.a);
    }
    return vec4<f32>(c, 1.0);
}

// A mip level from the one above it: each pixel the average of the 2×2
// above it, one bilinear sample at their shared corner.
@group(0) @binding(6) var finer: texture_2d<f32>;

@fragment
fn fs_mip(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = max(vec2<u32>(1u), textureDimensions(finer) >> vec2<u32>(1u));
    return textureSampleLevel(finer, bilinear, pos.xy / vec2<f32>(size), 0.0);
}
