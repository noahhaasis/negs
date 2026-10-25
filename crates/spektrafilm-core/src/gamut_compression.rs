//! Output gamut compression — port of upstream spektrafilm's
//! `utils/gamut_compression.compress_rgb` (oklch / oklrab / cam16ucs / aces_rgc).
//!
//! The scan produces linear RGB in the output color space (sRGB). Colors
//! outside the output cube would otherwise be hard-clipped to `[0, 1]` at
//! encode. The `oklch` compressor instead reduces *perceptual chroma*
//! smoothly: linear RGB → XYZ → OkLab → OkLch `(L, C, h)`, look up the
//! max in-gamut chroma `C_max(L, h)` for the output cube, apply a Reinhard
//! knee to `C / C_max`, and reconstruct — preserving OkLab hue and
//! lightness. A one-sided lightness compression first rolls super-bright
//! highlights (`L > white`) back into range.
//!
//! Ported: `oklch` (the default), `oklrab`, `cam16ucs` (full CIECAM16), and
//! `aces_rgc` (per-channel), for every output colourspace of
//! `spektrafilm_math::colorspace`. `jzazbz` remains a follow-up.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use spektrafilm_math::colorspace::RgbColourspace;
use spektrafilm_math::colorspace_data::{
    CAM16_C, CAM16_F_L, CAM16_N, CAM16_N_BB, CAM16_N_C, CAM16_N_CB, CAM16_Z, CAT16 as CAM16_M16,
    CAT16_INVERSE as CAM16_M16I,
};

use crate::params::OutputGamutCompressParams;

// OkLab (Ottosson) matrices, exactly as colour-science stores them.
const M1_XYZ_TO_LMS: [[f64; 3]; 3] = [
    [0.8189330101, 0.3618667424, -0.1288597137],
    [0.0329845436, 0.9293118715, 0.0361456387],
    [0.0482003018, 0.2643662691, 0.633851707],
];
const M1_LMS_TO_XYZ: [[f64; 3]; 3] = [
    [1.2270138511035211, -0.5577999806518222, 0.2812561489664678],
    [-0.0405801784232806, 1.11225686961683, -0.07167667866560119],
    [-0.0763812845057069, -0.4214819784180127, 1.5861632204407947],
];
const M2_LMS_TO_LAB: [[f64; 3]; 3] = [
    [0.2104542553, 0.793617785, -0.0040720468],
    [1.9779984951, -2.428592205, 0.4505937099],
    [0.0259040371, 0.7827717662, -0.808675766],
];
const M2_LAB_TO_LMS: [[f64; 3]; 3] = [
    [0.9999999984505196, 0.3963377921737678, 0.21580375806075877],
    [
        1.0000000088817607,
        -0.10556134232365633,
        -0.0638541747717059,
    ],
    [
        1.0000000546724108,
        -0.08948418209496574,
        -1.2914855378640917,
    ],
];

// C_max(L, h) table grid dims, matching `_OKLCH_CMAX_TABLE_N_*`. The L-axis
// bounds and the bisection's chroma ceiling are per-space (see `build`).
const N_L: usize = 64;
const N_H: usize = 720;
const N_BISECT: usize = 18;

#[inline]
fn mat_vec(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

#[inline]
fn xyz_to_oklab(xyz: [f64; 3]) -> [f64; 3] {
    let lms = mat_vec(&M1_XYZ_TO_LMS, xyz);
    let lms_ = [lms[0].cbrt(), lms[1].cbrt(), lms[2].cbrt()];
    mat_vec(&M2_LMS_TO_LAB, lms_)
}

#[inline]
fn oklab_to_xyz(lab: [f64; 3]) -> [f64; 3] {
    let lms_ = mat_vec(&M2_LAB_TO_LMS, lab);
    let lms = [lms_[0].powi(3), lms_[1].powi(3), lms_[2].powi(3)];
    mat_vec(&M1_LMS_TO_XYZ, lms)
}

// Ottosson 2023 "Lr": a 1D monotonic remap of OkLab L so the lightness scale
// tracks CIELAB L* more closely. Lr(0)=0, Lr(1)=1.
const OKLRAB_K1: f64 = 0.206;
const OKLRAB_K2: f64 = 0.03;
const OKLRAB_K3: f64 = (1.0 + OKLRAB_K1) / (1.0 + OKLRAB_K2);

#[inline]
fn oklab_l_to_lr(l: f64) -> f64 {
    let t = OKLRAB_K3 * l - OKLRAB_K1;
    0.5 * (t + (t * t + 4.0 * OKLRAB_K2 * OKLRAB_K3 * l).sqrt())
}

#[inline]
fn oklrab_lr_to_l(lr: f64) -> f64 {
    (lr * (lr + OKLRAB_K1)) / (OKLRAB_K3 * (lr + OKLRAB_K2))
}

/// Smooth Reinhard knee on normalized distance: identity below `threshold`,
/// asymptotic at `limit` above it. Matches the ACES RGC v1.3 reference.
#[inline]
pub(crate) fn reinhard_knee(d: f64, threshold: f64, limit: f64, power: f64) -> f64 {
    if d > threshold {
        let scale = limit - threshold;
        let x = (d - threshold) / scale;
        let y = x / (1.0 + x.powf(power)).powf(1.0 / power);
        threshold + scale * y
    } else {
        d
    }
}

#[inline]
fn h_grid(j: usize) -> f64 {
    // linspace(-pi, pi, N_H, endpoint=False)
    -std::f64::consts::PI + (2.0 * std::f64::consts::PI) * (j as f64) / (N_H as f64)
}

/// Perceptual space the chroma reduction runs in.
#[derive(Clone, Copy, PartialEq)]
enum Space {
    Oklch,
    /// OkLab with the Ottosson Lr-rebased lightness on the L axis.
    Oklrab,
    Cam16ucs,
}

impl Space {
    /// linear output RGB → perceptual reconstruction coords `[L, a, b]`.
    ///
    /// Upstream converts with `RGB_to_XYZ`/`XYZ_to_RGB(illuminant=whitepoint)`,
    /// whose CAT02 from the whitepoint to itself is the identity to ~1e-16,
    /// so only the colourspace's matrices are applied.
    #[inline]
    fn from_rgb(self, cs: &RgbColourspace, rgb: [f64; 3]) -> [f64; 3] {
        let xyz = mat_vec(&cs.rgb_to_xyz, rgb);
        match self {
            // oklrab shares OkLab coords; only the C_max lookup index differs.
            Space::Oklch | Space::Oklrab => xyz_to_oklab(xyz),
            Space::Cam16ucs => xyz_to_cam16ucs(cs, xyz),
        }
    }
    /// perceptual reconstruction coords `[L, a, b]` → linear output RGB.
    #[inline]
    fn to_rgb(self, cs: &RgbColourspace, lab: [f64; 3]) -> [f64; 3] {
        let xyz = match self {
            Space::Oklch | Space::Oklrab => oklab_to_xyz(lab),
            Space::Cam16ucs => cam16ucs_to_xyz(cs, lab),
        };
        mat_vec(&cs.xyz_to_rgb, xyz)
    }
    /// Reconstruction lightness `L` → C_max-table lookup index. Identity except
    /// oklrab, which indexes the table by Ottosson's rebased `Lr`.
    #[inline]
    fn lookup_lightness(self, l: f64) -> f64 {
        match self {
            Space::Oklrab => oklab_l_to_lr(l),
            _ => l,
        }
    }
    /// C_max-table lookup index → reconstruction lightness `L` (for the bake).
    #[inline]
    fn recon_lightness(self, lookup: f64) -> f64 {
        match self {
            Space::Oklrab => oklrab_lr_to_l(lookup),
            _ => lookup,
        }
    }
    /// `(l_min, l_max, chroma_upper)` — the C_max table's lightness-axis
    /// bounds and the bisection's chroma ceiling, fixed per space as
    /// upstream's `_get_output_c_max_table` fixes them for every output.
    fn table_geometry(self) -> (f64, f64, f64) {
        match self {
            Space::Oklch | Space::Oklrab => (0.02, 1.0, 0.5),
            Space::Cam16ucs => (1.0, 110.0, 150.0),
        }
    }
}

/// Which algorithm the compressor runs.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Off,
    /// ACES Reference Gamut Compression — per-channel knee in RGB, no table.
    AcesRgc,
    /// Perceptual chroma reduction against a `C_max(L, h)` table.
    Perceptual(Space),
}

/// Pre-computed output gamut compressor for a fixed output color space.
#[derive(Clone)]
pub struct OutputGamutCompress {
    kind: Kind,
    /// The output colourspace whose cube the compressor targets.
    output: &'static RgbColourspace,
    knee: (f64, f64, f64),
    lightness: Option<(f64, f64, f64)>,
    /// Perceptual lightness of the output white (1.0 for OkLab, ~100 for CAM16-UCS).
    l_white: f64,
    /// Lightness-axis bounds of the `C_max` table grid.
    l_min: f64,
    l_max: f64,
    /// `C_max(L, h)` table, row-major `[N_L][N_H]`. Empty for off/aces_rgc.
    /// Shared via the process-wide cache — the table depends only on the
    /// perceptual space and the output colourspace (the knee/lightness
    /// params don't enter the bake),
    /// and the ~105 ms bisection would otherwise re-run on every pipeline
    /// rebuild (the GUI rebuilds the pipeline per slider change).
    cmax: Arc<Vec<f64>>,
}

impl OutputGamutCompress {
    /// No-op (identity) compressor.
    pub fn identity() -> Self {
        Self {
            kind: Kind::Off,
            output: &spektrafilm_math::colorspace::SRGB,
            knee: (0.0, 1.0, 1.0),
            lightness: None,
            l_white: 1.0,
            l_min: 0.0,
            l_max: 1.0,
            cmax: Arc::new(Vec::new()),
        }
    }

    pub fn is_active(&self) -> bool {
        self.kind != Kind::Off
    }

    /// Build from params for the given output color space (unknown names:
    /// sRGB, as the scan's output). `off` is a silent identity; the
    /// perceptual algorithms build their `C_max` table for that output's cube.
    /// `jzazbz` (not yet ported) and unknown algorithms log an error and fall
    /// back to identity (so colors pass through uncompressed rather than
    /// silently appearing to be handled).
    pub fn build(params: &OutputGamutCompressParams, output_color_space: &str) -> Self {
        let output = RgbColourspace::output(output_color_space);
        let knee = (
            params.knee[0] as f64,
            params.knee[1] as f64,
            params.knee[2] as f64,
        );
        let lightness = params
            .lightness_compression
            .map(|l| (l[0] as f64, l[1] as f64, l[2] as f64));
        // aces_rgc is per-channel in RGB (no perceptual space / table / lightness).
        if params.algorithm == "aces_rgc" {
            return Self {
                kind: Kind::AcesRgc,
                output,
                knee,
                lightness: None,
                l_white: 1.0,
                l_min: 0.0,
                l_max: 1.0,
                cmax: Arc::new(Vec::new()),
            };
        }
        let space = match params.algorithm.as_str() {
            "off" => return Self::identity(),
            "oklch" => Space::Oklch,
            "oklrab" => Space::Oklrab,
            "cam16ucs" => Space::Cam16ucs,
            other @ "jzazbz" => {
                tracing::error!(
                    algorithm = other,
                    "output gamut compression: jzazbz not yet ported; disabling"
                );
                return Self::identity();
            }
            other => {
                tracing::error!(
                    algorithm = other,
                    "output gamut compression: unknown algorithm; disabling"
                );
                return Self::identity();
            }
        };
        // Perceptual white: OkLab white sits at L=1; CAM16-UCS Jp at the output
        // white (~100 under the configured viewing conditions). The white is
        // the exact whitepoint chromaticity (matching Python's
        // `_output_cs_whitepoint_xyz` → `_xy_to_xyz_unit_y`), NOT the rounded
        // matrix's column sums.
        let l_white = match space {
            // OkLab white sits at L=1, and Lr(1)=1, so both are 1.0.
            Space::Oklch | Space::Oklrab => 1.0,
            Space::Cam16ucs => xyz_to_cam16ucs(output, output.whitepoint_xyz_unit_y)[0],
        };
        let (l_min, l_max, _) = space.table_geometry();
        let cmax = cmax_table_cached(space, output);
        Self {
            kind: Kind::Perceptual(space),
            output,
            knee,
            lightness,
            l_white,
            l_min,
            l_max,
            cmax,
        }
    }

    /// GPU-pass inputs for the active compressor (mode id matching
    /// `gamut_compress.wgsl`, knee/lightness coefficients, table geometry
    /// and the baked `C_max` table). `None` when the compressor is off —
    /// callers skip the pass entirely.
    pub fn gpu_params(&self) -> Option<spektrafilm_gpu::GamutGpuParams<'_>> {
        let mode = match self.kind {
            Kind::Off => return None,
            Kind::AcesRgc => 0,
            Kind::Perceptual(Space::Oklch) => 1,
            Kind::Perceptual(Space::Oklrab) => 2,
            Kind::Perceptual(Space::Cam16ucs) => 3,
        };
        Some(spektrafilm_gpu::GamutGpuParams {
            mode,
            knee: [self.knee.0 as f32, self.knee.1 as f32, self.knee.2 as f32],
            lightness: self
                .lightness
                .map(|(t, l, p)| [t as f32, l as f32, p as f32]),
            l_white: self.l_white as f32,
            l_min: self.l_min as f32,
            l_max: self.l_max as f32,
            cmax: &self.cmax,
            n_l: N_L as u32,
            n_h: N_H as u32,
            rgb_to_xyz: spektrafilm_math::colorspace::mat_f32(self.output.rgb_to_xyz),
            xyz_to_rgb: spektrafilm_math::colorspace::mat_f32(self.output.xyz_to_rgb),
            cam16_d_rgb: self.output.cam16_d_rgb.map(|v| v as f32),
            cam16_a_w: self.output.cam16_a_w as f32,
        })
    }

    /// Bilinear `C_max(L, h)` lookup (L clamped, h wraps).
    #[inline]
    fn c_max_lookup(&self, l: f64, h: f64) -> f64 {
        let l = l.clamp(self.l_min, self.l_max);
        let h_step = h_grid(1) - h_grid(0);
        let h_idx = (h - h_grid(0)) / h_step;
        let h_floor = h_idx.floor();
        let h_lo = (h_floor as i64).rem_euclid(N_H as i64) as usize;
        let h_hi = (h_lo + 1) % N_H;
        let h_frac = h_idx - h_floor;

        let l_idx = (l - self.l_min) / (self.l_max - self.l_min) * ((N_L - 1) as f64);
        let l_lo = (l_idx.floor() as i64).clamp(0, (N_L - 2) as i64) as usize;
        let l_hi = l_lo + 1;
        let l_frac = l_idx - l_lo as f64;

        let t = &self.cmax;
        let v00 = t[l_lo * N_H + h_lo];
        let v01 = t[l_lo * N_H + h_hi];
        let v10 = t[l_hi * N_H + h_lo];
        let v11 = t[l_hi * N_H + h_hi];
        v00 * (1.0 - l_frac) * (1.0 - h_frac)
            + v01 * (1.0 - l_frac) * h_frac
            + v10 * l_frac * (1.0 - h_frac)
            + v11 * l_frac * h_frac
    }

    /// Compress a single linear output-RGB pixel. Identity when inactive.
    #[inline]
    pub fn compress(&self, rgb: [f64; 3]) -> [f64; 3] {
        let space = match self.kind {
            Kind::Off => return rgb,
            Kind::AcesRgc => return self.compress_aces_rgc(rgb),
            Kind::Perceptual(s) => s,
        };
        let lab = space.from_rgb(self.output, rgb);
        // One-sided lightness compression first (so C_max is looked up at the
        // corrected L), normalized by the perceptual white.
        let mut l = lab[0];
        if let Some((t, lim, p)) = self.lightness {
            l = reinhard_knee(l / self.l_white, t, lim, p) * self.l_white;
        }
        let a = lab[1];
        let b = lab[2];
        let c = a.hypot(b);
        let h = b.atan2(a);
        // C_max is looked up at the lookup-lightness (Lr for oklrab); the
        // reconstruction below keeps the reconstruction lightness `l`.
        let c_max = self.c_max_lookup(space.lookup_lightness(l), h);
        let safe = c_max.max(1e-9);
        let d_comp = reinhard_knee(c / safe, self.knee.0, self.knee.1, self.knee.2);
        let c_new = d_comp * safe;
        space.to_rgb(self.output, [l, c_new * h.cos(), c_new * h.sin()])
    }

    /// ACES Reference Gamut Compression v1.3: per-channel Reinhard knee on the
    /// achromatic distance `d = (max - c)/max`. Mirrors `compress_rgb_aces_rgc`.
    #[inline]
    fn compress_aces_rgc(&self, rgb: [f64; 3]) -> [f64; 3] {
        let ach = rgb[0].max(rgb[1]).max(rgb[2]);
        if ach <= 1e-12 {
            return rgb;
        }
        let mut out = [0.0f64; 3];
        for c in 0..3 {
            let d = (ach - rgb[c]) / ach;
            let dc = reinhard_knee(d, self.knee.0, self.knee.1, self.knee.2);
            out[c] = ach * (1.0 - dc);
        }
        out
    }
}

// ─────────────────────────────────────────────────────────────────────────
// CAM16-UCS (upstream's default `cam16ucs` method).
//
// Fixed viewing conditions: XYZ_w = output colourspace white, L_A = 64 cd/m²,
// Y_b = 20, Average surround. The white-independent quantities, and each
// colourspace's `D_RGB` and `A_w`, are generated from colour-science
// (`spektrafilm_math::colorspace_data`). The per-pixel path is the CIECAM16
// forward/inverse + the Luo 2006 UCS transform.
// XYZ here is at Y=1; CIECAM16 works in the Y=100 domain, hence the ×/÷100.
// ─────────────────────────────────────────────────────────────────────────
// Luo 2006 CAM16-UCS coefficients (K_L=1.0, c1, c2).
const UCS_C1: f64 = 0.007;
const UCS_C2: f64 = 0.0228;

/// colour's `spow`, `sign(a) * |a| ** p`, which CIECAM02/16 use so that
/// out-of-range responses stay finite instead of NaN.
#[inline]
fn spow(a: f64, p: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    let sign = if a > 0.0 {
        1.0
    } else if a < 0.0 {
        -1.0
    } else {
        0.0
    };
    sign * a.abs().powf(p)
}

#[inline]
fn atan2_deg(y: f64, x: f64) -> f64 {
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

#[inline]
fn cam16_padc_forward(rgb: [f64; 3]) -> [f64; 3] {
    let mut out = [0.0f64; 3];
    for i in 0..3 {
        let flr = (CAM16_F_L * rgb[i].abs() / 100.0).powf(0.42);
        out[i] = 400.0 * rgb[i].signum() * flr / (27.13 + flr) + 0.1;
    }
    out
}

#[inline]
fn cam16_padc_inverse(rgb: [f64; 3]) -> [f64; 3] {
    let mut out = [0.0f64; 3];
    for i in 0..3 {
        let d = rgb[i] - 0.1;
        let base = (27.13 * d.abs()) / (400.0 - d.abs());
        out[i] = d.signum() * 100.0 / CAM16_F_L * spow(base, 1.0 / 0.42);
    }
    out
}

/// XYZ (Y=1) → CAM16-UCS `(Jp, ap, bp)`, adapted to `cs`'s whitepoint.
fn xyz_to_cam16ucs(cs: &RgbColourspace, xyz: [f64; 3]) -> [f64; 3] {
    let (d_rgb, a_w) = (cs.cam16_d_rgb, cs.cam16_a_w);
    let xyz100 = [xyz[0] * 100.0, xyz[1] * 100.0, xyz[2] * 100.0];
    let rgb = mat_vec(&CAM16_M16, xyz100);
    let rgb_c = [rgb[0] * d_rgb[0], rgb[1] * d_rgb[1], rgb[2] * d_rgb[2]];
    let [ra, ga, ba] = cam16_padc_forward(rgb_c);
    let a = ra - 12.0 * ga / 11.0 + ba / 11.0;
    let b = (ra + ga - 2.0 * ba) / 9.0;
    let h = atan2_deg(b, a);
    let e_t = 0.25 * ((2.0 + h * std::f64::consts::PI / 180.0).cos() + 3.8);
    let a_resp = (2.0 * ra + ga + ba / 20.0 - 0.305) * CAM16_N_BB;
    let jj = 100.0 * spow(a_resp / a_w, CAM16_C * CAM16_Z);
    let denom = ra + ga + 21.0 * ba / 20.0;
    let t = if denom != 0.0 {
        (50000.0 / 13.0) * CAM16_N_C * CAM16_N_CB * (e_t * (a * a + b * b).sqrt()) / denom
    } else {
        0.0
    };
    let cc = spow(t, 0.9) * spow(jj / 100.0, 0.5) * spow(1.64 - 0.29f64.powf(CAM16_N), 0.73);
    let m = cc * CAM16_F_L.powf(0.25);
    let jp = (1.0 + 100.0 * UCS_C1) * jj / (1.0 + UCS_C1 * jj);
    let mp = (1.0 / UCS_C2) * (1.0 + UCS_C2 * m).ln();
    let hr = h * std::f64::consts::PI / 180.0;
    [jp, mp * hr.cos(), mp * hr.sin()]
}

/// colour's `sdiv` in its default "Ignore Zero Conversion" mode:
/// `np.nan_to_num(a / b)`, so NaN becomes 0 and ±inf the largest finite value.
#[inline]
fn sdiv(a: f64, b: f64) -> f64 {
    let q = a / b;
    if q.is_nan() {
        0.0
    } else {
        q.clamp(f64::MIN, f64::MAX)
    }
}

/// CAM16-UCS `(Jp, ap, bp)` → XYZ (Y=1), adapted to `cs`'s whitepoint.
fn cam16ucs_to_xyz(cs: &RgbColourspace, jab: [f64; 3]) -> [f64; 3] {
    let (d_rgb, a_w) = (cs.cam16_d_rgb, cs.cam16_a_w);
    let [jp, ap, bp] = jab;
    let mp = (ap * ap + bp * bp).sqrt();
    let h = atan2_deg(bp, ap);
    let jj = jp / ((1.0 + 100.0 * UCS_C1) - UCS_C1 * jp);
    let m = ((UCS_C2 * mp).exp() - 1.0) / UCS_C2;
    let cc = m / CAM16_F_L.powf(0.25);
    // `np.maximum`, which keeps NaN.
    let j_prime = if jj.is_nan() { jj } else { jj.max(f64::EPSILON) };
    let t = spow(
        cc / ((j_prime / 100.0).sqrt() * spow(1.64 - 0.29f64.powf(CAM16_N), 0.73)),
        1.0 / 0.9,
    );
    let e_t = 0.25 * ((2.0 + h * std::f64::consts::PI / 180.0).cos() + 3.8);
    let a_resp = a_w * spow(jj / 100.0, 1.0 / (CAM16_C * CAM16_Z));
    let p1 = sdiv((50000.0 / 13.0) * CAM16_N_C * CAM16_N_CB * e_t, t);
    let p2 = a_resp / CAM16_N_BB + 0.305;
    let p3 = 21.0 / 20.0;
    let (mut a, mut b) = cam16_opponent_inverse(p1, p2, p3, h);
    // Achromatic guard, matching colour's `ab * np.where(t == 0, 0, 1)`:
    // when t == 0 the hue is undefined and the opponent inverse must be zeroed.
    if t == 0.0 {
        a = 0.0;
        b = 0.0;
    }
    let ra = (460.0 * p2 + 451.0 * a + 288.0 * b) / 1403.0;
    let ga = (460.0 * p2 - 891.0 * a - 261.0 * b) / 1403.0;
    let ba = (460.0 * p2 - 220.0 * a - 6300.0 * b) / 1403.0;
    let rgb_c = cam16_padc_inverse([ra, ga, ba]);
    let rgb = [rgb_c[0] / d_rgb[0], rgb_c[1] / d_rgb[1], rgb_c[2] / d_rgb[2]];
    let xyz100 = mat_vec(&CAM16_M16I, rgb);
    [xyz100[0] / 100.0, xyz100[1] / 100.0, xyz100[2] / 100.0]
}

/// Inverse opponent dimensions (CIECAM02/16 `opponent_colour_dimensions_inverse`).
/// A NaN hue (chroma lost in the forward pass, e.g. for negative luminance)
/// fails both of colour's branch masks and comes back achromatic.
fn cam16_opponent_inverse(p1: f64, p2: f64, p3: f64, h: f64) -> (f64, f64) {
    let hr = h * std::f64::consts::PI / 180.0;
    let s = hr.sin();
    let c = hr.cos();
    let (cos_sin, sin_cos) = (sdiv(c, s), sdiv(s, c));
    let nn = p2 * (2.0 + p3) * (460.0 / 1403.0);
    if s.abs() >= c.abs() {
        let p4 = sdiv(p1, s);
        let b = nn
            / (p4 + (2.0 + p3) * (220.0 / 1403.0) * cos_sin - (27.0 / 1403.0)
                + p3 * (6300.0 / 1403.0));
        (b * cos_sin, b)
    } else if s.abs() < c.abs() {
        let p5 = sdiv(p1, c);
        let a = nn
            / (p5 + (2.0 + p3) * (220.0 / 1403.0)
                - ((27.0 / 1403.0) - p3 * (6300.0 / 1403.0)) * sin_cos);
        (a, a * sin_cos)
    } else {
        (0.0, 0.0)
    }
}

/// Process-wide `C_max` table cache, keyed by perceptual space and output
/// colourspace. The bake's other inputs come from `Space::table_geometry`.
/// The build runs outside the lock — a racing double-build is harmless and
/// cheaper than serializing on it.
fn cmax_table_cached(space: Space, cs: &'static RgbColourspace) -> Arc<Vec<f64>> {
    type Cache = Mutex<HashMap<(u8, &'static str), Arc<Vec<f64>>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let key = match space {
        Space::Oklch => 0u8,
        Space::Oklrab => 1,
        Space::Cam16ucs => 2,
    };
    let key = (key, cs.name);
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(table) = cache.lock().unwrap().get(&key) {
        return table.clone();
    }
    let table = Arc::new(build_cmax_table(space, cs));
    cache.lock().unwrap().insert(key, table.clone());
    table
}

/// Bisect the max in-gamut chroma at each `(L, h)` grid node for the given
/// perceptual space and output cube, mirroring
/// `_build_polar_perceptual_c_max_table`. Built once per pair (see
/// `cmax_table_cached`).
fn build_cmax_table(space: Space, cs: &RgbColourspace) -> Vec<f64> {
    use rayon::prelude::*;
    let (l_min, l_max, chroma_upper) = space.table_geometry();
    let mut table = vec![0.0f64; N_L * N_H];
    table
        .par_chunks_exact_mut(N_H)
        .enumerate()
        .for_each(|(i, row)| {
            // Grid value is the lookup-lightness; map to reconstruction
            // lightness (Lr→L for oklrab) before the gamut check.
            let lookup_l = l_min + (l_max - l_min) * (i as f64) / ((N_L - 1) as f64);
            let l = space.recon_lightness(lookup_l);
            for (j, out) in row.iter_mut().enumerate() {
                let h = h_grid(j);
                let (cos_h, sin_h) = (h.cos(), h.sin());
                let mut lo = 0.0f64;
                let mut hi = chroma_upper;
                for _ in 0..N_BISECT {
                    let mid = (lo + hi) * 0.5;
                    let rgb = space.to_rgb(cs, [l, mid * cos_h, mid * sin_h]);
                    let in_gamut = rgb.iter().all(|&v| v >= -1e-6 && v <= 1.0 + 1e-6);
                    if in_gamut {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                *out = lo;
            }
        });
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_for(algorithm: &str) -> OutputGamutCompress {
        let params = OutputGamutCompressParams {
            algorithm: algorithm.into(),
            knee: [0.0, 1.0, 6.0],
            lightness_compression: Some([0.7, 1.0, 2.2]),
        };
        OutputGamutCompress::build(&params, "sRGB")
    }

    // Full cross-checks against upstream (tmp/gen_oklch_ref.py,
    // tmp/gen_cam16_ref.py) confirmed bit-identical C_max tables and a
    // per-pixel compress matching Python to ~1.6e-8 over 100+ in/out-of-gamut
    // rows for both spaces, and the CAM16-UCS conversion matching colour to
    // ~3e-15 (forward) with a ~4e-15 roundtrip. The reference rows below pin
    // that in CI without depending on the generated dumps.

    fn check_reference(algo: &str, cases: &[([f64; 3], [f64; 3])]) {
        let comp = build_for(algo);
        for (rgb, want) in cases {
            let got = comp.compress(*rgb);
            for c in 0..3 {
                // 1e-6 bound: bit-faithful tables; the per-pixel chain agrees
                // with Python to ~1.6e-8 at extreme out-of-gamut points
                // (release-build FMA reordering), far below the threshold.
                assert!(
                    (got[c] - want[c]).abs() < 1e-6,
                    "{algo} rgb {rgb:?} ch {c}: got {} want {}",
                    got[c],
                    want[c]
                );
            }
        }
    }

    /// Regression guard: oklch reference rows from `compress_rgb_oklch_chroma`
    /// (knee (0,1,6), lightness (0.7,1,2.2)) — in-gamut, super-red, super-cyan.
    #[test]
    fn oklch_matches_python_reference() {
        check_reference(
            "oklch",
            &[
                (
                    [0.5, 0.4, 0.3],
                    [0.49934573392726778, 0.3994604511652195, 0.29954075929970825],
                ),
                (
                    [2.0, 0.0, 0.0],
                    [0.9999165297352135, 0.32648626944154618, 0.2608847299103626],
                ),
                (
                    [0.0, 2.0, 2.0],
                    [0.50013028382005009, 1.0000925606064377, 0.98946014302066698],
                ),
            ],
        );
    }

    /// Regression guard: cam16ucs reference rows from
    /// `compress_rgb_cam16ucs_chroma` (same knee/lightness).
    #[test]
    fn cam16ucs_matches_python_reference() {
        check_reference(
            "cam16ucs",
            &[
                (
                    [0.5, 0.4, 0.3],
                    [
                        0.49974636928661886,
                        0.39981273339722234,
                        0.29983897633446865,
                    ],
                ),
                (
                    [2.0, 0.0, 0.0],
                    [0.9987900431158222, 0.32032525739267781, 0.25154805990152163],
                ),
                (
                    [0.0, 2.0, 2.0],
                    [
                        0.46476658163581974,
                        0.99783501836142674,
                        0.99071804859295698,
                    ],
                ),
            ],
        );
    }

    /// Regression guard: oklrab reference rows from `compress_rgb_oklrab_chroma`.
    #[test]
    fn oklrab_matches_python_reference() {
        check_reference(
            "oklrab",
            &[
                (
                    [0.5, 0.4, 0.3],
                    [0.4993457339279313, 0.39946045116508211, 0.29954075929887197],
                ),
                (
                    [2.0, 0.0, 0.0],
                    [
                        0.99981431200564952,
                        0.32651963747538049,
                        0.26092249322038208,
                    ],
                ),
                (
                    [0.0, 2.0, 2.0],
                    [0.49984508410893458, 1.0001915827542378, 0.98955691468555185],
                ),
            ],
        );
    }

    /// Regression guard: aces_rgc reference rows from `compress_rgb_aces_rgc`
    /// (per-channel knee, no lightness/table).
    #[test]
    fn aces_rgc_matches_python_reference() {
        check_reference(
            "aces_rgc",
            &[
                (
                    [0.5, 0.4, 0.3],
                    [0.5, 0.40000106662684631, 0.30013620807161223],
                ),
                (
                    [2.0, 0.0, 0.0],
                    [2.0, 0.21820256371932145, 0.21820256371932145],
                ),
                (
                    [1.2, -0.1, 0.4],
                    [1.2, 0.092551484339614021, 0.41114161919196679],
                ),
            ],
        );
    }

    /// `off` (and unported algorithms) must be a true identity no-op.
    #[test]
    fn off_is_identity() {
        let params = OutputGamutCompressParams {
            algorithm: "off".into(),
            ..Default::default()
        };
        let comp = OutputGamutCompress::build(&params, "sRGB");
        assert!(!comp.is_active());
        assert_eq!(comp.compress([1.7, 0.0, 0.3]), [1.7, 0.0, 0.3]);
    }
}
