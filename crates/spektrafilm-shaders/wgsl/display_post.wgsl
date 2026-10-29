// Post adjustments in the display pass: `PostProcess::apply_rgb`
// (crates/spektrafilm-core/src/post.rs) with the same tables and the same
// f32 arithmetic, so a presented frame matches the CPU's adjusted scan.
// Appended to the display shaders, which declare `post` (a `Post`) and
// `post_tables` (decode, encode and tone, `POST_LUT` values each) at their
// own bindings.

const POST_LUT: u32 = 4096u;

struct Post {
    on: u32,
    encoded: u32,
    saturation: f32,
    _pad: u32,
    luma: vec4<f32>,
}

// `lookup` in post.rs: linear interpolation in table `t` (0 decode,
// 1 encode, 2 tone), sampled on [0, 1].
fn post_lookup(t: u32, x: f32) -> f32 {
    let at = clamp(x, 0.0, 1.0) * f32(POST_LUT - 1u);
    let i = min(u32(at), POST_LUT - 2u);
    let f = at - f32(i);
    let a = post_tables[t * POST_LUT + i];
    let b = post_tables[t * POST_LUT + i + 1u];
    return a + (b - a) * f;
}

// A scan pixel, NaN-free and clipped to [0, 1], adjusted; in the scan's
// encoding, clipped too.
fn post_rgb(rgb: vec3<f32>) -> vec3<f32> {
    if post.on == 0u {
        return rgb;
    }
    var c = rgb;
    if post.encoded == 1u {
        c = vec3<f32>(post_lookup(0u, c.r), post_lookup(0u, c.g), post_lookup(0u, c.b));
    }
    let y = post.luma.r * c.r + post.luma.g * c.g + post.luma.b * c.b;
    let y2 = post_lookup(2u, sqrt(max(y, 0.0)));
    if y <= 1e-9 {
        c = vec3<f32>(y2);
    } else {
        c = c * (y2 / y);
        // Above white, give up saturation rather than luminance or hue.
        let top = max(c.r, max(c.g, c.b));
        if top > 1.0 {
            let k = (1.0 - y2) / (top - y2);
            c = y2 + (c - y2) * k;
        }
    }
    var f = 1.0 + post.saturation;
    if f > 1.0 {
        // Stop at the gamut boundary, keeping the hue.
        for (var i = 0u; i < 3u; i++) {
            let d = c[i] - y2;
            if d > 0.0 {
                f = min(f, (1.0 - y2) / d);
            } else if d < 0.0 {
                f = min(f, y2 / -d);
            }
        }
    }
    c = clamp(y2 + (c - y2) * f, vec3<f32>(0.0), vec3<f32>(1.0));
    if post.encoded == 1u {
        c = vec3<f32>(post_lookup(1u, sqrt(c.r)), post_lookup(1u, sqrt(c.g)), post_lookup(1u, sqrt(c.b)));
    }
    return c;
}
