// Draws the scan to a canvas of its size, one fragment per pixel, as the
// bytes `display_rgba8.wgsl` makes: the canvas is an 8-bit unorm surface,
// so byte / 255 stores the byte itself.

struct Params {
    width: u32,
    encoded: u32,
    _pad: vec2<u32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> rgb: array<f32>;

fn display_byte(v: f32) -> f32 {
    // NaN by its bits: compilers may fold `v != v` away.
    var x = select(v, 0.0, (bitcast<u32>(v) & 0x7fffffffu) > 0x7f800000u);
    x = clamp(x, 0.0, 1.0);
    if params.encoded == 0u {
        x = select(1.055 * pow(x, 1.0 / 2.4) - 0.055, 12.92 * x, x <= 0.0031308);
    }
    return floor(x * 255.0 + 0.5) / 255.0;
}

// One triangle over the whole viewport.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let base = (u32(pos.y) * params.width + u32(pos.x)) * 3u;
    return vec4<f32>(display_byte(rgb[base]), display_byte(rgb[base + 1u]), display_byte(rgb[base + 2u]), 1.0);
}
