// One radix-R stage of a Stockham autosort FFT (radix 2, 3, 4 or 5) along
// one axis of a complex image, for every line at once. Ping-pongs src → dst.
//
// Stage with radix R after stages whose radices multiply to ns: for
// j < n/R, x_k = src[j + k·n/R]·w^(k·(j mod ns)·n/(ns·R)), y = DFT_R(x),
// dst[(j / ns)·ns·R + j mod ns + k·ns] = y_k. After all stages the output
// is in natural order. Twiddles come from a host table exp(−2πi·m/n)
// computed in f64 (conjugated for the inverse); no trigonometry here.
//
// Used by the diffusion-filter FFT convolution (see `FftConvolution`).

struct Params {
    n: u32,            // transform length
    radix: u32,
    ns: u32,           // product of the radices of the earlier stages
    lines: u32,        // number of independent transforms
    elem_stride: u32,  // distance between consecutive elements of a line
    line_stride: u32,  // distance between the first elements of two lines
    inverse: u32,
    line_fastest: u32, // 1: consecutive threads take consecutive lines
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> src: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read_write> dst: array<vec2<f32>>;
@group(0) @binding(3) var<storage, read> twiddle: array<vec2<f32>>;

fn cmul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}

// i·z
fn rot(z: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(-z.y, z.x);
}

@compute @workgroup_size(256)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let t = gid.y * groups.x * 256u + gid.x;
    let radix = params.radix;
    let per_line = params.n / radix;
    if t >= per_line * params.lines {
        return;
    }
    var j: u32;
    var line: u32;
    if params.line_fastest != 0u {
        line = t % params.lines;
        j = t / params.lines;
    } else {
        j = t % per_line;
        line = t / per_line;
    }
    let base = line * params.line_stride;
    let es = params.elem_stride;
    let jm = j % params.ns;
    let tw_step = jm * (params.n / (params.ns * radix));
    // Sign of the exponent: −1 forward, +1 inverse.
    var sgn = -1.0;
    if params.inverse != 0u {
        sgn = 1.0;
    }

    var v: array<vec2<f32>, 5>;
    for (var k = 0u; k < radix; k++) {
        var x = src[base + (j + k * per_line) * es];
        if k > 0u {
            var w = twiddle[k * tw_step]; // exp(−2πi·m/n)
            if params.inverse != 0u {
                w.y = -w.y;
            }
            x = cmul(x, w);
        }
        v[k] = x;
    }

    var y: array<vec2<f32>, 5>;
    if radix == 2u {
        y[0] = v[0] + v[1];
        y[1] = v[0] - v[1];
    } else if radix == 4u {
        let a = v[0] + v[2];
        let b = v[0] - v[2];
        let c = v[1] + v[3];
        let d = sgn * rot(v[1] - v[3]); // ∓i·(v1 − v3)
        y[0] = a + c;
        y[1] = b + d;
        y[2] = a - c;
        y[3] = b - d;
    } else if radix == 3u {
        let t1 = v[1] + v[2];
        let m = v[0] - 0.5 * t1;
        let r = (sgn * 0.86602540378443864676) * rot(v[1] - v[2]);
        y[0] = v[0] + t1;
        y[1] = m + r;
        y[2] = m - r;
    } else {
        // radix 5, direct: y_k = Σ_m v_m·exp(sgn·2πi·k·m/5)
        let c = array<f32, 5>(1.0, 0.30901699437494742410, -0.80901699437494742410, -0.80901699437494742410, 0.30901699437494742410);
        let s = array<f32, 5>(0.0, 0.95105651629515357212, 0.58778525229247312917, -0.58778525229247312917, -0.95105651629515357212);
        for (var k = 0u; k < 5u; k++) {
            var acc = vec2<f32>(0.0);
            for (var m = 0u; m < 5u; m++) {
                let e = (k * m) % 5u;
                acc += cmul(v[m], vec2<f32>(c[e], sgn * s[e]));
            }
            y[k] = acc;
        }
    }

    let idx = (j / params.ns) * params.ns * radix + jm;
    for (var k = 0u; k < radix; k++) {
        dst[base + (idx + k * params.ns) * es] = y[k];
    }
}
